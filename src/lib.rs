extern crate rand;

use std::net::UdpSocket;
use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::mpsc;
use std::thread;

use std::time::Duration;

const TIMEOUT: u64 = 30;

pub fn reply_to_client(main_receiver: Receiver<(SocketAddr, Vec<u8>)>, responder: UdpSocket) {
    thread::spawn(move || {
        loop {
            let (dest, buf) = main_receiver.recv().unwrap();
            let to_send = buf.as_slice();
            responder
                .send_to(to_send, dest)
                .expect(&format!("Failed to forward response from upstream server to client {}",
                                 dest));
        }
    });
}

fn upstream_to_local(
    upstream_recv: UdpSocket,
    local_send_queue: Sender<(SocketAddr, Vec<u8>)>,
    src_addr: SocketAddr,
    local_timed_out: Arc<AtomicBool>)
{
    thread::spawn(move|| {
        let mut from_upstream = [0; 64 * 1024];
        upstream_recv.set_read_timeout(Some(Duration::from_millis(TIMEOUT + 100))).unwrap();
        loop {
            match upstream_recv.recv_from(&mut from_upstream) {
                Ok((bytes_rcvd, _)) => {
                    let to_send = from_upstream[..bytes_rcvd].to_vec();
                    local_send_queue.send((src_addr, to_send))
                        .expect("Failed to queue response from upstream server for forwarding!");
                },
                Err(_) => {
                    if local_timed_out.load(Ordering::Relaxed) {
                        break;
                    }
                }
            };
        }
    });
}

fn client_to_upstream(
    receiver: Receiver<Vec<u8>>,
    upstream_send: UdpSocket,
    timeouts: &mut u64,
    remote_addr_copy: String,
    src_addr: SocketAddr,
    timed_out: Arc<AtomicBool>,
) {
    loop {
        match receiver.recv_timeout(Duration::from_millis(TIMEOUT)) {
            Ok(from_client) => {
                upstream_send.send_to(from_client.as_slice(), &remote_addr_copy)
                    .expect(&format!("Failed to forward packet from client {} to upstream server!", src_addr));
                *timeouts = 0; //reset timeout count
            },
            Err(_) => {
                *timeouts += 1;
                if *timeouts >= 10 {
                    timed_out.store(true, Ordering::Relaxed);
                    break;
                }
            }
        };
    }
}

pub struct Forwarder {
    send_queue: Sender<(SocketAddr, Vec<u8>)>,
    remote_addr: String,
    src_addr: SocketAddr,
    sender: Sender<Vec<u8>>,
}

impl Forwarder {
    pub fn new(
        local_send_queue: Sender<(SocketAddr, Vec<u8>)>,
        remote_addr_copy: String,
        src_addr: SocketAddr,
    ) -> Forwarder {
        let send_q = local_send_queue.clone();
        let remote_a = remote_addr_copy.clone();
        let (sender, receiver) = channel::<Vec<u8>>();
        thread::spawn(move|| {
            //regardless of which port we are listening to, we don't know which interface or IP
            //address the remote server is reachable via, so we bind the outgoing
            //connection to 0.0.0.0 in all cases.
            let temp_outgoing_addr = format!("0.0.0.0:{}", 1024 + rand::random::<u16>());
            let upstream_send = UdpSocket::bind(&temp_outgoing_addr)
                .expect(&format!("Failed to bind to transient address {}", &temp_outgoing_addr));
            let upstream_recv = upstream_send.try_clone()
                .expect("Failed to clone client-specific connection to upstream!");

            let mut timeouts : u64 = 0;
            let timed_out = Arc::new(AtomicBool::new(false));

            let local_timed_out = timed_out.clone();
            upstream_to_local(upstream_recv,
                              send_q,
                              src_addr,
                              local_timed_out,
            );

            client_to_upstream(receiver, upstream_send, & mut timeouts, remote_a, src_addr, timed_out);

        });
        Forwarder {
            send_queue: local_send_queue,
            remote_addr: remote_addr_copy,
            src_addr: src_addr,
            sender: sender
        }
    }

    pub fn send(&self, data: Vec<u8>)-> Result<(), mpsc::SendError<Vec<u8>>> {
        self.sender.send(data)
    }
}

