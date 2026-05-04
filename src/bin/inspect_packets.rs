use std::fs::File;
use std::io::{Cursor, Read};

use bson::{Bson, Document};

fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "packets.bin".to_string());
    let mut file = File::open(&path).map_err(|error| format!("open {path}: {error}"))?;
    let mut record_index = 0usize;

    loop {
        let mut header = [0u8; 9];
        match file.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(format!("read header: {error}")),
        }

        let direction = match header[0] {
            b'S' => "send",
            b'R' => "recv",
            other => {
                return Err(format!(
                    "record {record_index}: unknown direction byte {other:#x}"
                ));
            }
        };
        let timestamp = u32::from_le_bytes(header[1..5].try_into().unwrap());
        let len = u32::from_le_bytes(header[5..9].try_into().unwrap()) as usize;
        let mut payload = vec![0u8; len];
        file.read_exact(&mut payload)
            .map_err(|error| format!("read payload for record {record_index}: {error}"))?;

        let outer = Document::from_reader(Cursor::new(payload))
            .map_err(|error| format!("record {record_index}: {error}"))?;
        let messages = extract_messages(&outer);
        let mc = outer.get_i32("mc").unwrap_or(-1);
        if messages.is_empty() {
            println!("#{record_index} {direction} ts={timestamp} [empty batch mc={mc}]");
        } else {
            for (message_index, message) in messages.iter().enumerate() {
                println!(
                    "#{record_index}.{message_index} {direction} ts={timestamp} mc={mc} {}",
                    summarize_message(message)
                );
            }
        }

        record_index += 1;
    }

    Ok(())
}

fn extract_messages(outer: &Document) -> Vec<Document> {
    let count = outer.get_i32("mc").unwrap_or_default().max(0) as usize;
    let mut messages = Vec::with_capacity(count);
    for index in 0..count {
        if let Some(Bson::Document(message)) = outer.get(&format!("m{index}")) {
            messages.push(message.clone());
        }
    }
    if messages.is_empty() && outer.contains_key("ID") {
        messages.push(outer.clone());
    }
    messages
}

fn summarize_message(message: &Document) -> String {
    let id = message.get_str("ID").unwrap_or("?");
    let mut parts = vec![format!("ID={id}")];
    for (key, value) in message {
        if key == "ID" {
            continue;
        }
        parts.push(format!("{key}={}", render_bson(value)));
    }
    parts.join(" ")
}

fn render_bson(value: &Bson) -> String {
    match value {
        Bson::String(value) => value.clone(),
        Bson::Int32(value) => value.to_string(),
        Bson::Int64(value) => value.to_string(),
        Bson::Double(value) => format!("{value:.2}"),
        Bson::Boolean(value) => value.to_string(),
        Bson::Binary(value) => format!("<binary:{}B>", value.bytes.len()),
        Bson::Array(items) => {
            let rendered = items.iter().map(render_bson).collect::<Vec<_>>().join(",");
            format!("[{rendered}]")
        }
        Bson::Document(document) => {
            let rendered = document
                .iter()
                .map(|(key, value)| format!("{key}:{}", render_bson(value)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{rendered}}}")
        }
        other => format!("{other:?}"),
    }
}
