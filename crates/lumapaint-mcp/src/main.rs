use std::io::{self, BufRead, Write};
fn main() -> io::Result<()> {
    let mut server = lumapaint_mcp::Server::default();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        // Bound allocation before parsing; an oversized frame closes the session.
        let mut line = Vec::new();
        loop {
            let buffer = input.fill_buf()?;
            if buffer.is_empty() {
                return Ok(());
            }
            let count = buffer
                .iter()
                .position(|b| *b == b'\n')
                .map_or(buffer.len(), |p| p + 1);
            if line.len() + count > 65536 {
                eprintln!("MCP frame exceeds 65536 bytes");
                return Ok(());
            }
            line.extend_from_slice(&buffer[..count]);
            input.consume(count);
            if line.last() == Some(&b'\n') {
                break;
            }
        }
        let reply = match serde_json::from_slice(&line) {
            Ok(message) => server.handle(message),
            Err(_) => Some(
                serde_json::json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            ),
        };
        if let Some(reply) = reply {
            serde_json::to_writer(&mut output, &reply)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}
