use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_STATE_DIR: &str = ".cache/polyrhythm";
const DEFAULT_SINK: &str = "alsa_output.pci-0000_0e_00.4.analog-stereo";
const DEFAULT_SAFETY_BUS: &str = "TD50-Safety-Bus";
const DEFAULT_MIDI_SOURCE: &str = "Midi-Bridge:TD50-DrumGizmo-Hihat-Mapperout (capture)";
const DEFAULT_MIDI_TARGET: &str = "DrumGizmo:drumgizmo_midiin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSnapshot {
    pub path: PathBuf,
    pub objects: usize,
    pub nodes: BTreeMap<u32, Node>,
    pub ports: BTreeMap<u32, Port>,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: u32,
    pub name: String,
    pub media_class: Option<String>,
    pub application: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    pub id: u32,
    pub node_id: u32,
    pub name: String,
    pub alias: Option<String>,
    pub direction: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub id: u32,
    pub output_port_id: u32,
    pub input_port_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSummary {
    pub drumgizmo_running: bool,
    pub midi_link_present: bool,
    pub drumgizmo_audio_connections: Vec<String>,
    pub sink_inputs: Vec<String>,
    pub obs_inputs: Vec<String>,
    pub suspicious: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesiredState {
    EngineOnly,
    OverheadMonitor,
}

pub fn dump() -> io::Result<GraphSnapshot> {
    let state_dir = state_dir();
    let graph_dir = state_dir.join("graphs");
    fs::create_dir_all(&graph_dir)?;
    let path = graph_dir.join(format!("{}.json", run_id()));
    let output = Command::new("timeout").arg("2s").arg("pw-dump").output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "pw-dump failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    fs::write(&path, &output.stdout)?;
    parse_snapshot(path, &String::from_utf8_lossy(&output.stdout))
}

pub fn parse_snapshot(path: PathBuf, text: &str) -> io::Result<GraphSnapshot> {
    let mut parser = Parser::new(text);
    let value = parser.parse_value()?;
    let Value::Array(objects) = value else {
        return Err(io::Error::other("pw-dump root is not an array"));
    };

    let mut nodes = BTreeMap::new();
    let mut ports = BTreeMap::new();
    let mut links = Vec::new();
    let objects_len = objects.len();

    for object in objects {
        let Value::Object(map) = object else { continue };
        let ty = map.get("type").and_then(Value::as_str).unwrap_or_default();
        let id = map.get("id").and_then(Value::as_u32).unwrap_or_default();
        let props = map
            .get("info")
            .and_then(Value::as_object)
            .and_then(|info| info.get("props"))
            .and_then(Value::as_object);
        if ty.ends_with(":Node") {
            let name = prop(props, "node.name")
                .or_else(|| prop(props, "object.path"))
                .unwrap_or_else(|| id.to_string());
            nodes.insert(
                id,
                Node {
                    id,
                    name,
                    media_class: prop(props, "media.class"),
                    application: prop(props, "application.name"),
                    state: map
                        .get("info")
                        .and_then(Value::as_object)
                        .and_then(|info| info.get("state"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                },
            );
        } else if ty.ends_with(":Port") {
            let node_id = prop(props, "node.id")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or_default();
            let name = prop(props, "port.name").unwrap_or_else(|| id.to_string());
            ports.insert(
                id,
                Port {
                    id,
                    node_id,
                    name,
                    alias: prop(props, "port.alias"),
                    direction: prop(props, "port.direction"),
                },
            );
        } else if ty.ends_with(":Link") {
            let info = map.get("info").and_then(Value::as_object);
            let output_port_id = info
                .and_then(|info| info.get("output-port-id"))
                .and_then(Value::as_u32)
                .unwrap_or_default();
            let input_port_id = info
                .and_then(|info| info.get("input-port-id"))
                .and_then(Value::as_u32)
                .unwrap_or_default();
            links.push(Link {
                id,
                output_port_id,
                input_port_id,
            });
        }
    }

    Ok(GraphSnapshot {
        path,
        objects: objects_len,
        nodes,
        ports,
        links,
    })
}

pub fn summarize(snapshot: &GraphSnapshot) -> GraphSummary {
    let drumgizmo_running = snapshot
        .nodes
        .values()
        .any(|node| node.name == "DrumGizmo" && node.state.as_deref() == Some("running"));
    let mut drumgizmo_audio_connections = Vec::new();
    let mut sink_inputs = Vec::new();
    let mut obs_inputs = Vec::new();
    let mut midi_link_present = false;

    for link in &snapshot.links {
        let Some(out) = snapshot.ports.get(&link.output_port_id) else {
            continue;
        };
        let Some(input) = snapshot.ports.get(&link.input_port_id) else {
            continue;
        };
        let out_node = snapshot
            .nodes
            .get(&out.node_id)
            .map(|node| node.name.as_str())
            .unwrap_or("?");
        let in_node = snapshot
            .nodes
            .get(&input.node_id)
            .map(|node| node.name.as_str())
            .unwrap_or("?");
        let rendered = format!("{out_node}:{} -> {in_node}:{}", out.name, input.name);
        if rendered == format!("{DEFAULT_MIDI_SOURCE} -> {DEFAULT_MIDI_TARGET}") {
            midi_link_present = true;
        }
        if out_node == "DrumGizmo"
            && input.direction.as_deref() == Some("in")
            && input.name.starts_with("playback_")
        {
            drumgizmo_audio_connections.push(rendered.clone());
        }
        if in_node == DEFAULT_SINK && input.name.starts_with("playback_") {
            sink_inputs.push(rendered.clone());
        }
        if in_node == "OBS" && input.name.starts_with("input_") {
            obs_inputs.push(rendered);
        }
    }

    let mut suspicious = Vec::new();
    for input in &obs_inputs {
        if input.starts_with(&format!("{DEFAULT_SINK}:monitor_")) {
            suspicious.push(format!("OBS is receiving speaker monitor feed: {input}"));
        }
    }

    GraphSummary {
        drumgizmo_running,
        midi_link_present,
        drumgizmo_audio_connections,
        sink_inputs,
        obs_inputs,
        suspicious,
    }
}

pub fn check(snapshot: &GraphSnapshot, desired: DesiredState) -> Vec<String> {
    let summary = summarize(snapshot);
    let mut failures = Vec::new();
    if !summary.drumgizmo_running {
        failures.push("DrumGizmo node is not running".to_string());
    }
    if !summary.midi_link_present {
        failures.push("required MIDI link is missing".to_string());
    }
    match desired {
        DesiredState::EngineOnly => {
            if !summary.drumgizmo_audio_connections.is_empty() {
                failures.push(format!(
                    "expected DrumGizmo audio disconnected, found: {}",
                    summary.drumgizmo_audio_connections.join("; ")
                ));
            }
            failures.extend(summary.suspicious);
        }
        DesiredState::OverheadMonitor => {
            let required_drum_to_bus = [
                format!("DrumGizmo:5-OHL -> {DEFAULT_SAFETY_BUS}:playback_FL"),
                format!("DrumGizmo:6-OHR -> {DEFAULT_SAFETY_BUS}:playback_FR"),
            ];
            for route in required_drum_to_bus {
                if !summary
                    .drumgizmo_audio_connections
                    .iter()
                    .any(|input| input == &route)
                {
                    failures.push(format!("missing overhead safety-bus route: {route}"));
                }
            }
            let required_bus_to_sink = [
                format!("{DEFAULT_SAFETY_BUS}:monitor_FL -> {DEFAULT_SINK}:playback_FL"),
                format!("{DEFAULT_SAFETY_BUS}:monitor_FR -> {DEFAULT_SINK}:playback_FR"),
            ];
            for route in required_bus_to_sink {
                if !summary.sink_inputs.iter().any(|input| input == &route) {
                    failures.push(format!("missing safety-bus monitor route: {route}"));
                }
            }
            for route in &summary.drumgizmo_audio_connections {
                if !route.starts_with("DrumGizmo:5-OHL -> TD50-Safety-Bus:")
                    && !route.starts_with("DrumGizmo:6-OHR -> TD50-Safety-Bus:")
                {
                    failures.push(format!("unexpected DrumGizmo monitor route: {route}"));
                }
                if route.contains(&format!("-> {DEFAULT_SINK}:")) {
                    failures.push(format!("unsafe direct DrumGizmo sink route: {route}"));
                }
            }
        }
    }
    failures
}

pub fn print_summary(snapshot: &GraphSnapshot) {
    let summary = summarize(snapshot);
    println!("graph snapshot: {}", snapshot.path.display());
    println!("objects: {}", snapshot.objects);
    println!(
        "nodes: {} ports: {} links: {}",
        snapshot.nodes.len(),
        snapshot.ports.len(),
        snapshot.links.len()
    );
    println!("drumgizmo running: {}", summary.drumgizmo_running);
    println!("required midi link: {}", summary.midi_link_present);
    println!("DrumGizmo audio connections:");
    if summary.drumgizmo_audio_connections.is_empty() {
        println!("  none");
    } else {
        for route in &summary.drumgizmo_audio_connections {
            println!("  {route}");
        }
    }
    println!("speaker sink inputs:");
    for route in &summary.sink_inputs {
        println!("  {route}");
    }
    println!("OBS inputs:");
    for route in &summary.obs_inputs {
        println!("  {route}");
    }
    if !summary.suspicious.is_empty() {
        println!("suspicious routes:");
        for route in &summary.suspicious {
            println!("  {route}");
        }
    }
}

fn prop(props: Option<&BTreeMap<String, Value>>, key: &str) -> Option<String> {
    props
        .and_then(|props| props.get(key))
        .and_then(Value::as_scalar_string)
}

fn state_dir() -> PathBuf {
    env::var_os("POLYRHYTHM_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(DEFAULT_STATE_DIR)))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_DIR))
}

fn run_id() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    fn as_u32(&self) -> Option<u32> {
        match self {
            Self::Number(value) => value.parse().ok(),
            Self::String(value) => value.parse().ok(),
            _ => None,
        }
    }

    fn as_scalar_string(&self) -> Option<String> {
        match self {
            Self::String(value) | Self::Number(value) => Some(value.clone()),
            Self::Bool(value) => Some(value.to_string()),
            Self::Null | Self::Array(_) | Self::Object(_) => None,
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    fn parse_value(&mut self) -> io::Result<Value> {
        self.skip_ws();
        let Some(byte) = self.peek() else {
            return Err(io::Error::other("unexpected end of JSON"));
        };
        match byte {
            b'n' => {
                self.expect_literal(b"null")?;
                Ok(Value::Null)
            }
            b't' => {
                self.expect_literal(b"true")?;
                Ok(Value::Bool(true))
            }
            b'f' => {
                self.expect_literal(b"false")?;
                Ok(Value::Bool(false))
            }
            b'"' => self.parse_string().map(Value::String),
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            b'-' | b'0'..=b'9' => self.parse_number().map(Value::Number),
            _ => Err(io::Error::other("unexpected JSON token")),
        }
    }

    fn parse_array(&mut self) -> io::Result<Value> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        loop {
            self.skip_ws();
            if self.consume(b']') {
                break;
            }
            values.push(self.parse_value()?);
            self.skip_ws();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(Value::Array(values))
    }

    fn parse_object(&mut self) -> io::Result<Value> {
        self.expect(b'{')?;
        let mut values = BTreeMap::new();
        loop {
            self.skip_ws();
            if self.consume(b'}') {
                break;
            }
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            let value = self.parse_value()?;
            values.insert(key, value);
            self.skip_ws();
            if self.consume(b'}') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(Value::Object(values))
    }

    fn parse_string(&mut self) -> io::Result<String> {
        self.expect(b'"')?;
        let mut output = String::new();
        while let Some(byte) = self.next() {
            match byte {
                b'"' => return Ok(output),
                b'\\' => {
                    let escaped = self
                        .next()
                        .ok_or_else(|| io::Error::other("unterminated escape"))?;
                    match escaped {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{0008}'),
                        b'f' => output.push('\u{000c}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => {
                            let mut code = 0u32;
                            for _ in 0..4 {
                                let digit = self
                                    .next()
                                    .ok_or_else(|| io::Error::other("bad unicode escape"))?;
                                code = code * 16
                                    + (digit as char)
                                        .to_digit(16)
                                        .ok_or_else(|| io::Error::other("bad unicode escape"))?;
                            }
                            if let Some(ch) = char::from_u32(code) {
                                output.push(ch);
                            }
                        }
                        _ => return Err(io::Error::other("bad escape")),
                    }
                }
                _ => output.push(byte as char),
            }
        }
        Err(io::Error::other("unterminated string"))
    }

    fn parse_number(&mut self) -> io::Result<String> {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'-' | b'+' | b'.' | b'0'..=b'9' | b'e' | b'E')
        ) {
            self.pos += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.pos])
            .map(ToOwned::to_owned)
            .map_err(io::Error::other)
    }

    fn expect_literal(&mut self, literal: &[u8]) -> io::Result<()> {
        for byte in literal {
            self.expect(*byte)?;
        }
        Ok(())
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: u8) -> io::Result<()> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(io::Error::other("unexpected JSON byte"))
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_extracts_graph_links() {
        let json = r#"[
          {"id":1,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"DrumGizmo"},"state":"running"}},
          {"id":2,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"Midi-Bridge"},"state":"running"}},
          {"id":3,"type":"PipeWire:Interface:Port","info":{"props":{"node.id":1,"port.name":"drumgizmo_midiin","port.direction":"in"}}},
          {"id":4,"type":"PipeWire:Interface:Port","info":{"props":{"node.id":2,"port.name":"TD50-DrumGizmo-Hihat-Mapperout (capture)","port.direction":"out"}}},
          {"id":5,"type":"PipeWire:Interface:Link","info":{"output-port-id":4,"input-port-id":3}}
        ]"#;
        let snapshot = parse_snapshot(PathBuf::from("test.json"), json).unwrap();
        let summary = summarize(&snapshot);
        assert!(summary.drumgizmo_running);
        assert!(summary.midi_link_present);
    }
}
