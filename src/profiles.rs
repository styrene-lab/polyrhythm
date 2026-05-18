use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceProfile {
    pub id: String,
    pub name: String,
    pub inputs: Vec<DeviceInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInput {
    pub id: String,
    pub intent: Intent,
    pub notes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitProfile {
    pub id: String,
    pub name: String,
    pub kit_xml: PathBuf,
    pub sample_params: String,
    pub mappings: BTreeMap<Intent, KitTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitTarget {
    pub note: u8,
    pub instrument: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Intent {
    KickMain,
    SnareHead,
    SnareRim,
    SnareRimshot,
    TomHead(u8),
    TomRim(u8),
    HihatClosed,
    HihatSemiOpen,
    HihatOpen,
    HihatPedal,
    RideBow,
    RideBell,
    RideEdge,
    CrashBow(u8),
    CrashEdge(u8),
    CrashChoke(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedMidimap {
    pub xml: String,
    pub mapped: Vec<GeneratedMapping>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedMapping {
    pub intent: Intent,
    pub device_note: u8,
    pub kit_note: u8,
    pub instrument: String,
}

pub fn builtin_devices(repo: &Path) -> Vec<DeviceProfile> {
    load_devices(repo).unwrap_or_default()
}

pub fn builtin_kits(repo: &Path, home: &Path) -> Vec<KitProfile> {
    load_kits(repo, home).unwrap_or_default()
}

pub fn find_device(repo: &Path, id: &str) -> Option<DeviceProfile> {
    builtin_devices(repo)
        .into_iter()
        .find(|device| device.id == id)
}

pub fn find_kit(repo: &Path, home: &Path, id: &str) -> Option<KitProfile> {
    builtin_kits(repo, home)
        .into_iter()
        .find(|kit| kit.id == id)
}

pub fn load_devices(repo: &Path) -> Result<Vec<DeviceProfile>, String> {
    let dir = repo.join("profiles/devices");
    let mut profiles = Vec::new();
    for path in pikl_files(&dir)? {
        profiles.push(parse_device_profile(&read_profile(&path)?)?);
    }
    profiles.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(profiles)
}

pub fn load_kits(repo: &Path, home: &Path) -> Result<Vec<KitProfile>, String> {
    let dir = repo.join("profiles/kits");
    let mut profiles = Vec::new();
    for path in pikl_files(&dir)? {
        profiles.push(parse_kit_profile(&read_profile(&path)?, home)?);
    }
    profiles.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(profiles)
}

pub fn generate_midimap(device: &DeviceProfile, kit: &KitProfile) -> GeneratedMidimap {
    let mut mapped = Vec::new();
    let mut warnings = Vec::new();
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<midimap>\n");

    let mut emitted = BTreeSet::new();
    for input in &device.inputs {
        let Some(target) = kit.mappings.get(&input.intent) else {
            warnings.push(format!(
                "unmapped intent {} from input {}",
                input.intent, input.id
            ));
            continue;
        };
        for note in &input.notes {
            if emitted.insert((*note, target.instrument.clone())) {
                let _ = writeln!(
                    xml,
                    "  <map note=\"{}\" instr=\"{}\"/>",
                    note, target.instrument
                );
            }
            mapped.push(GeneratedMapping {
                intent: input.intent.clone(),
                device_note: *note,
                kit_note: target.note,
                instrument: target.instrument.clone(),
            });
        }
    }

    xml.push_str("</midimap>\n");
    GeneratedMidimap {
        xml,
        mapped,
        warnings,
    }
}

pub fn generated_midimap_path(
    cache_dir: &Path,
    device: &DeviceProfile,
    kit: &KitProfile,
) -> PathBuf {
    cache_dir
        .join("generated-midimaps")
        .join(format!("{}-{}.xml", device.id, kit.id))
}

pub fn write_generated_midimap(
    cache_dir: &Path,
    device: &DeviceProfile,
    kit: &KitProfile,
) -> std::io::Result<(PathBuf, GeneratedMidimap)> {
    let generated = generate_midimap(device, kit);
    let path = generated_midimap_path(cache_dir, device, kit);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, &generated.xml)?;
    Ok((path, generated))
}

fn pikl_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for entry in
        fs::read_dir(dir).map_err(|err| format!("failed to read {}: {err}", dir.display()))?
    {
        let path = entry
            .map_err(|err| format!("failed to read {} entry: {err}", dir.display()))?
            .path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("pikl") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn read_profile(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("failed to read {}: {err}", path.display()))
}

fn parse_device_profile(text: &str) -> Result<DeviceProfile, String> {
    let mut lines = logical_lines(text);
    let header = lines
        .next()
        .ok_or_else(|| "device profile is empty".to_string())?;
    let id = parse_block_header(&header, "device")?;
    let mut name = id.clone();
    let mut inputs = Vec::new();
    for line in lines {
        if line == "}" {
            break;
        } else if let Some(raw) = line.strip_prefix("name ") {
            name = parse_quoted(raw)?;
        } else if line.starts_with("input ") {
            inputs.push(parse_input_line(&line)?);
        } else {
            return Err(format!("unsupported device profile line: {line}"));
        }
    }
    Ok(DeviceProfile { id, name, inputs })
}

fn parse_kit_profile(text: &str, home: &Path) -> Result<KitProfile, String> {
    let mut lines = logical_lines(text);
    let header = lines
        .next()
        .ok_or_else(|| "kit profile is empty".to_string())?;
    let id = parse_block_header(&header, "kit")?;
    let mut name = id.clone();
    let mut kit_xml = PathBuf::new();
    let mut sample_params = String::new();
    let mut mappings = BTreeMap::new();
    for line in lines {
        if line == "}" {
            break;
        } else if let Some(raw) = line.strip_prefix("name ") {
            name = parse_quoted(raw)?;
        } else if let Some(raw) = line.strip_prefix("kit_xml ") {
            kit_xml = expand_home(&parse_quoted(raw)?, home);
        } else if let Some(raw) = line.strip_prefix("sample_params ") {
            sample_params = parse_quoted(raw)?;
        } else if line.starts_with("map ") {
            let (intent, target) = parse_map_line(&line)?;
            mappings.insert(intent, target);
        } else {
            return Err(format!("unsupported kit profile line: {line}"));
        }
    }
    Ok(KitProfile {
        id,
        name,
        kit_xml,
        sample_params,
        mappings,
    })
}

fn logical_lines(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().filter_map(|line| {
        let line = line.split('#').next().unwrap_or_default().trim();
        (!line.is_empty()).then(|| line.to_string())
    })
}

fn parse_block_header(line: &str, kind: &str) -> Result<String, String> {
    let prefix = format!("{kind} ");
    let rest = line
        .strip_prefix(&prefix)
        .ok_or_else(|| format!("expected {kind} profile header, got: {line}"))?;
    let id = rest
        .strip_suffix('{')
        .ok_or_else(|| format!("expected opening block in: {line}"))?
        .trim();
    if id.is_empty() {
        Err(format!("empty {kind} id"))
    } else {
        Ok(id.to_string())
    }
}

fn parse_input_line(line: &str) -> Result<DeviceInput, String> {
    let rest = line.strip_prefix("input ").unwrap();
    let (id, body) = rest
        .split_once('{')
        .ok_or_else(|| format!("expected input block: {line}"))?;
    let body = body
        .trim()
        .strip_suffix('}')
        .ok_or_else(|| format!("expected closing input block: {line}"))?
        .trim();
    let tokens = tokenize(body);
    let intent_index = tokens
        .iter()
        .position(|token| token == "intent")
        .ok_or_else(|| format!("input missing intent: {line}"))?;
    let notes_index = tokens
        .iter()
        .position(|token| token == "notes")
        .ok_or_else(|| format!("input missing notes: {line}"))?;
    let intent = parse_intent(
        tokens
            .get(intent_index + 1)
            .ok_or_else(|| format!("input missing intent value: {line}"))?,
    )?;
    let notes = parse_notes(
        tokens
            .get(notes_index + 1)
            .ok_or_else(|| format!("input missing notes value: {line}"))?,
    )?;
    Ok(DeviceInput {
        id: id.trim().to_string(),
        intent,
        notes,
    })
}

fn parse_map_line(line: &str) -> Result<(Intent, KitTarget), String> {
    let tokens = tokenize(line);
    if tokens.len() != 6 || tokens[0] != "map" || tokens[2] != "note" || tokens[4] != "instr" {
        return Err(format!("invalid map line: {line}"));
    }
    let intent = parse_intent(&tokens[1])?;
    let note = tokens[3]
        .parse()
        .map_err(|_| format!("invalid map note in: {line}"))?;
    Ok((
        intent,
        KitTarget {
            note,
            instrument: tokens[5].clone(),
        },
    ))
}

fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut bracket_depth = 0;
    for ch in line.chars() {
        match ch {
            '[' => {
                bracket_depth += 1;
                current.push(ch);
            }
            ']' => {
                bracket_depth -= 1;
                current.push(ch);
            }
            ch if ch.is_whitespace() && bracket_depth == 0 => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn parse_notes(raw: &str) -> Result<Vec<u8>, String> {
    let raw = raw
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| format!("notes must be bracketed: {raw}"))?;
    raw.split(',')
        .map(|note| {
            note.trim()
                .parse()
                .map_err(|_| format!("invalid note: {note}"))
        })
        .collect()
}

fn parse_quoted(raw: &str) -> Result<String, String> {
    raw.trim()
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(ToString::to_string)
        .ok_or_else(|| format!("expected quoted string: {raw}"))
}

fn expand_home(raw: &str, home: &Path) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    }
}

fn parse_intent(raw: &str) -> Result<Intent, String> {
    match raw {
        "kick.main" => Ok(Intent::KickMain),
        "snare.head" => Ok(Intent::SnareHead),
        "snare.rim" => Ok(Intent::SnareRim),
        "snare.rimshot" => Ok(Intent::SnareRimshot),
        "hihat.closed" => Ok(Intent::HihatClosed),
        "hihat.semi_open" => Ok(Intent::HihatSemiOpen),
        "hihat.open" => Ok(Intent::HihatOpen),
        "hihat.pedal" => Ok(Intent::HihatPedal),
        "ride.bow" => Ok(Intent::RideBow),
        "ride.bell" => Ok(Intent::RideBell),
        "ride.edge" => Ok(Intent::RideEdge),
        _ => {
            if let Some(index) = raw
                .strip_prefix("tom.")
                .and_then(|rest| rest.strip_suffix(".head"))
            {
                return index
                    .parse()
                    .map(Intent::TomHead)
                    .map_err(|_| format!("invalid tom intent: {raw}"));
            }
            if let Some(index) = raw
                .strip_prefix("tom.")
                .and_then(|rest| rest.strip_suffix(".rim"))
            {
                return index
                    .parse()
                    .map(Intent::TomRim)
                    .map_err(|_| format!("invalid tom intent: {raw}"));
            }
            if let Some(index) = raw
                .strip_prefix("crash.")
                .and_then(|rest| rest.strip_suffix(".bow"))
            {
                return index
                    .parse()
                    .map(Intent::CrashBow)
                    .map_err(|_| format!("invalid crash intent: {raw}"));
            }
            if let Some(index) = raw
                .strip_prefix("crash.")
                .and_then(|rest| rest.strip_suffix(".edge"))
            {
                return index
                    .parse()
                    .map(Intent::CrashEdge)
                    .map_err(|_| format!("invalid crash intent: {raw}"));
            }
            if let Some(index) = raw
                .strip_prefix("crash.")
                .and_then(|rest| rest.strip_suffix(".choke"))
            {
                return index
                    .parse()
                    .map(Intent::CrashChoke)
                    .map_err(|_| format!("invalid crash intent: {raw}"));
            }
            Err(format!("unknown intent: {raw}"))
        }
    }
}

impl std::fmt::Display for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Intent::KickMain => f.write_str("kick.main"),
            Intent::SnareHead => f.write_str("snare.head"),
            Intent::SnareRim => f.write_str("snare.rim"),
            Intent::SnareRimshot => f.write_str("snare.rimshot"),
            Intent::TomHead(index) => write!(f, "tom.{index}.head"),
            Intent::TomRim(index) => write!(f, "tom.{index}.rim"),
            Intent::HihatClosed => f.write_str("hihat.closed"),
            Intent::HihatSemiOpen => f.write_str("hihat.semi_open"),
            Intent::HihatOpen => f.write_str("hihat.open"),
            Intent::HihatPedal => f.write_str("hihat.pedal"),
            Intent::RideBow => f.write_str("ride.bow"),
            Intent::RideBell => f.write_str("ride.bell"),
            Intent::RideEdge => f.write_str("ride.edge"),
            Intent::CrashBow(index) => write!(f, "crash.{index}.bow"),
            Intent::CrashEdge(index) => write!(f, "crash.{index}.edge"),
            Intent::CrashChoke(index) => write!(f, "crash.{index}.choke"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_td50_device_profile_from_pikl() {
        let profile = parse_device_profile(include_str!("../profiles/devices/td50.pikl")).unwrap();
        assert_eq!(profile.id, "td50");
        assert!(profile
            .inputs
            .iter()
            .any(|input| input.intent == Intent::TomHead(2) && input.notes == [45]));
        assert!(profile
            .inputs
            .iter()
            .any(|input| input.intent == Intent::TomRim(2) && input.notes == [47]));
        assert!(profile
            .inputs
            .iter()
            .any(|input| input.intent == Intent::CrashEdge(1) && input.notes == [55, 81]));
    }

    #[test]
    fn parses_crocell_kit_profile_from_pikl() {
        let profile = parse_kit_profile(
            include_str!("../profiles/kits/crocell.pikl"),
            Path::new("/home/test"),
        )
        .unwrap();
        assert_eq!(profile.id, "crocell");
        assert_eq!(
            profile.kit_xml,
            Path::new("/home/test/.local/share/drumgizmo/kits/CrocellKit/CrocellKit_full.xml")
        );
        assert_eq!(
            profile
                .mappings
                .get(&Intent::TomHead(2))
                .unwrap()
                .instrument,
            "Tom2"
        );
    }

    #[test]
    fn td50_crocell_generation_maps_observed_tom_note_to_crocell_tom2() {
        let device = parse_device_profile(include_str!("../profiles/devices/td50.pikl")).unwrap();
        let kit = parse_kit_profile(
            include_str!("../profiles/kits/crocell.pikl"),
            Path::new("/home/test"),
        )
        .unwrap();
        let generated = generate_midimap(&device, &kit);
        assert!(generated.xml.contains("<map note=\"45\" instr=\"Tom2\"/>"));
        assert!(generated.xml.contains("<map note=\"43\" instr=\"FTom1\"/>"));
        assert!(generated.xml.contains("<map note=\"38\" instr=\"Snare\"/>"));
        assert!(generated.warnings.is_empty());
    }

    #[test]
    fn generated_path_is_device_kit_specific() {
        let device = parse_device_profile(include_str!("../profiles/devices/td50.pikl")).unwrap();
        let kit = parse_kit_profile(
            include_str!("../profiles/kits/crocell.pikl"),
            Path::new("/home/test"),
        )
        .unwrap();
        let path = generated_midimap_path(Path::new("/tmp/polyrhythm"), &device, &kit);
        assert_eq!(
            path,
            Path::new("/tmp/polyrhythm/generated-midimaps/td50-crocell.xml")
        );
    }
}
