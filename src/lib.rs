use std::net::UdpSocket;
use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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

pub fn upstream_to_local(
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
