use crate::{Error, Response};

use std::{io, net::Ipv4Addr};

use async_std::net::UdpSocket;
use async_stream::try_stream;
use futures_core::Stream;
use std::sync::Arc;

#[cfg(not(target_os = "windows"))]
use net2::unix::UnixUdpBuilderExt;
use std::net::SocketAddr;

/// The IP address for the mDNS multicast socket.
const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MULTICAST_PORT: u16 = 5353;

fn mdns_interface_inner(
    service_name: String,
    interface_addr: Ipv4Addr,
    with_loopback: bool,
) -> Result<(mDNSListener, mDNSSender), Error> {
    println!("CREATING SOCKET");
    let socket = create_socket()?;

    println!("SETTING MULTICAST LOOP V4, with loopback={}", with_loopback);
    socket.set_multicast_loop_v4(with_loopback)?;
    let use_interface_addr = if with_loopback { Ipv4Addr::LOCALHOST } else { interface_addr };
    println!("JOINING MULTICAST V4, with multicast multicast_addr={}, interface_addr={}", MULTICAST_ADDR, use_interface_addr);
    // socket.join_multicast_v4(&MULTICAST_ADDR, &interface_addr)?;
    socket.join_multicast_v4(&MULTICAST_ADDR, &use_interface_addr)?;

    println!("CREATING SOCKET WITH REFERENCE COUNTER");
    let socket = Arc::new(UdpSocket::from(socket));

    println!("CREATING BUFFER");
    let recv_buffer = vec![0; 4096];

    println!("RETURN MDNS LISTENER AND SENDER");
    Ok((
        mDNSListener {
            recv: socket.clone(),
            recv_buffer,
        },
        mDNSSender {
            service_name,
            send: socket,
        },
    ))
}

pub fn mdns_interface(
    service_name: String,
    interface_addr: Ipv4Addr,
) -> Result<(mDNSListener, mDNSSender), Error> {
    mdns_interface_inner(service_name, interface_addr, false)
}

pub fn mdns_interface_with_loopback(
    service_name: String,
    interface_addr: Ipv4Addr,
) -> Result<(mDNSListener, mDNSSender), Error> {
    mdns_interface_inner(service_name, interface_addr, true)
}

#[cfg(not(target_os = "windows"))]
fn create_socket() -> io::Result<std::net::UdpSocket> {
    net2::UdpBuilder::new_v4()?
        .reuse_address(true)?
        .reuse_port(true)?
        .bind((Ipv4Addr::UNSPECIFIED, MULTICAST_PORT))
}

#[cfg(target_os = "windows")]
fn create_socket() -> io::Result<std::net::UdpSocket> {
    net2::UdpBuilder::new_v4()?
        .reuse_address(true)?
        .bind((Ipv4Addr::UNSPECIFIED, MULTICAST_PORT))
}

/// An mDNS sender on a specific interface.
#[derive(Debug, Clone)]
#[allow(non_camel_case_types)]
pub struct mDNSSender {
    service_name: String,
    send: Arc<UdpSocket>,
}

impl mDNSSender {
    /// Send multicasted DNS queries.
    pub async fn send_request(&mut self) -> Result<(), Error> {
        let mut builder = dns_parser::Builder::new_query(0, false);
        let prefer_unicast = false;
        builder.add_question(
            &self.service_name,
            prefer_unicast,
            dns_parser::QueryType::PTR,
            dns_parser::QueryClass::IN,
        );
        let packet_data = builder.build().unwrap();

        let addr = SocketAddr::new(MULTICAST_ADDR.into(), MULTICAST_PORT);

        self.send.send_to(&packet_data, addr).await?;
        Ok(())
    }
}

/// An mDNS listener on a specific interface.
#[derive(Debug, Clone)]
#[allow(non_camel_case_types)]
pub struct mDNSListener {
    recv: Arc<UdpSocket>,
    recv_buffer: Vec<u8>,
}

impl mDNSListener {
    pub fn listen(mut self) -> impl Stream<Item = Result<Response, Error>> {
        try_stream! {
            loop {
                let (count, _) = self.recv.recv_from(&mut self.recv_buffer).await?;

                if count > 0 {
                    match dns_parser::Packet::parse(&self.recv_buffer[..count]) {
                        Ok(raw_packet) => yield Response::from_packet(&raw_packet),
                        Err(e) => log::warn!("{}, {:?}", e, &self.recv_buffer[..count]),
                    }
                }
            }
        }
    }
}
