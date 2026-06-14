use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};

pub fn start_metrics_server(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let handle = metrics_exporter_prometheus::PrometheusBuilder::new().install_recorder()?;

    let addr: SocketAddr = addr.parse()?;
    let listener = TcpListener::bind(addr)?;
    println!("metrics endpoint: http://{addr}/metrics");

    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            serve_one(&stream, &handle);
        }
    });

    Ok(())
}

fn serve_one(mut stream: &TcpStream, handle: &metrics_exporter_prometheus::PrometheusHandle) {
    let mut buf = [0u8; 2048];
    let _ = stream.read(&mut buf);
    let body = handle.render();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}
