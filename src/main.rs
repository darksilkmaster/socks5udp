extern crate getopts;
extern crate rand;
extern crate socks5udp;

use getopts::Options;
use std::collections::HashMap;
use std::env;
use std::net::UdpSocket;
use std::sync::mpsc::channel;

use socks5udp::reply_to_client;
use socks5udp::Forwarder;

static mut DEBUG: bool = false;

fn print_usage(program: &str, opts: Options) {
    let program_path = std::path::PathBuf::from(program);
    let program_name = program_path.file_stem().unwrap().to_str().unwrap();
    let brief = format!("Usage: {} [-b BIND_ADDR] -l LOCAL_PORT -h REMOTE_ADDR -r REMOTE_PORT",
                        program_name);
    print!("{}", opts.usage(&brief));
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.reqopt("l",
                "local-port",
                "The local port to which udpproxy should bind to",
                "LOCAL_PORT");
    opts.reqopt("r",
                "remote-port",
                "The remote port to which UDP packets should be forwarded",
                "REMOTE_PORT");
    opts.reqopt("h",
                "host",
                "The remote address to which packets will be forwarded",
                "REMOTE_ADDR");
    opts.optopt("b",
                "bind",
                "The address on which to listen for incoming requests",
                "BIND_ADDR");
    opts.optflag("d", "debug", "Enable debug mode");

    let matches = opts.parse(&args[1..])
        .unwrap_or_else(|_| {
                            print_usage(&program, opts);
                            std::process::exit(-1);
                        });

    unsafe {
        DEBUG = matches.opt_present("d");
    }
    let local_port: i32 = matches.opt_str("l").unwrap().parse().unwrap();
    let remote_port: i32 = matches.opt_str("r").unwrap().parse().unwrap();
    let remote_host = matches.opt_str("h").unwrap();
    let bind_addr = match matches.opt_str("b") {
        Some(addr) => addr,
        None => "127.0.0.1".to_owned(),
    };

    forward(&bind_addr, local_port, &remote_host, remote_port);
}

fn debug(msg: String) {
    let debug: bool;
    unsafe {
        debug = DEBUG;
    }

    if debug {
        println!("{}", msg);
    }
}

fn forward(bind_addr: &str, local_port: i32, remote_host: &str, remote_port: i32) {
    let local_addr = format!("{}:{}", bind_addr, local_port);
    let local = UdpSocket::bind(&local_addr).expect(&format!("Unable to bind to {}", &local_addr));
    println!("Listening on {}", local.local_addr().unwrap());

    let remote_addr = format!("{}:{}", remote_host, remote_port);

    let responder = local
        .try_clone()
        .expect(&format!("Failed to clone primary listening address socket {}",
                        local.local_addr().unwrap()));
    let (main_sender, main_receiver) = channel::<(_, Vec<u8>)>();
    reply_to_client(main_receiver, responder);

    let mut client_map = HashMap::new();
    let mut buf = [0; 64 * 1024];
    // loop to receive a new packet
    loop {
        let (num_bytes, src_addr) = local.recv_from(&mut buf).expect("Didn't receive data");

        //we create a new thread for each unique client
        // loop and send the packet
        let mut remove_existing = false;
        loop {
            debug(format!("Received packet from client {}", src_addr));

            let mut ignore_failure = true;
            let client_id = format!("{}", src_addr);

            if remove_existing {
                debug(format!("Removing existing forwarder from map."));
                client_map.remove(&client_id);
            }

            let sender = client_map.entry(client_id.clone())
                .or_insert_with(|| {
                    //we are creating a new listener now, so a failure to send shoud be treated as an error
                    ignore_failure = false;
                    Forwarder::new(
                        main_sender.clone(),
                        remote_addr.clone(),
                        src_addr,
                    )
                });

            let to_send = buf[..num_bytes].to_vec();
            match sender.send(to_send) {
                Ok(_) => {
                    break;
                }
                Err(_) => {
                    if !ignore_failure {
                        panic!("Failed to send message to datagram forwarder for client {}",
                               client_id);
                    }
                    //client previously timed out
                    debug(format!("New connection received from previously timed-out client {}",
                                  client_id));
                    remove_existing = true;
                    continue;
                }
            }
        }
    }
}
