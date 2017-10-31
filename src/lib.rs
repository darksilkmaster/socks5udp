use std::net::UdpSocket;
use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::thread;

pub fn local_to_remote(main_receiver: Receiver<(SocketAddr, Vec<u8>)>, responder: UdpSocket) {
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
