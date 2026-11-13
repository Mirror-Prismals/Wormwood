//! Cardinal USER/USER_SAMPLES discovery, cached on the control thread.
//! Accepted drops decode sources; cancellable drag workers stream preview envelopes.
//! Neither path performs filesystem work in the device callback.
use serde_json::{Value, json};
use std::{path::{Path, PathBuf}, time::{Duration, Instant}};

pub(crate) fn user_root(root: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("WORMWOOD_USER_ROOT") { return path.into(); }
    // Local packaged applications use the same editable library as cargo runs.
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("USER");
    if local.is_dir() { local } else { root.join("USER") }
}

#[derive(Default)]
pub(crate) struct Library {
    pub entries: Vec<Value>,
    checked: Option<Instant>,
    root: Option<PathBuf>,
}
impl Library {
    pub(crate) fn invalidate(&mut self) { self.checked = None; }
    pub(crate) fn refresh(&mut self, root: &Path) -> Result<(), String> {
        let root = user_root(root);
        if self.root.as_ref() == Some(&root) && self.checked.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) { return Ok(()); }
        self.checked = Some(Instant::now());
        self.root = Some(root.clone());
        self.entries = scan(&root)?;
        Ok(())
    }
}
pub(super) fn scan(root: &Path) -> Result<Vec<Value>, String> {
    let mut entries = Vec::new();
    // Also accept files directly under USER, including the original test tone.
    for dir in [root.to_owned(), root.join("USER_SAMPLES")] {
        let files = match std::fs::read_dir(&dir) {
            Ok(files) => files,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("Audio library {}: {e}", dir.display())),
        };
        for file in files.take(2048) {
            let file = file.map_err(|e| e.to_string())?;
            let path = file.path();
            if !file.file_type().map_err(|e| e.to_string())?.is_file()
                || !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) { continue; }
            entries.push(json!({"name":file.file_name().to_string_lossy(),"format":"WAV","path":path}));
        }
    }
    entries.sort_by(|a,b| a["name"].as_str().cmp(&b["name"].as_str()).then_with(||a["path"].as_str().cmp(&b["path"].as_str())));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_only_wavs_in_user_and_samples_sorted() {
        let root=std::env::temp_dir().join(format!("wormwood-audio-library-{}",std::process::id()));
        std::fs::create_dir_all(root.join("USER_SAMPLES")).unwrap();
        std::fs::write(root.join("z.wav"), []).unwrap();
        std::fs::write(root.join("USER_SAMPLES/a.WAV"), []).unwrap();
        std::fs::write(root.join("ignored.txt"), []).unwrap();
        let entries=scan(&root).unwrap();
        assert_eq!(entries.len(),2); assert_eq!(entries[0]["name"],"a.WAV");
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// One cancellable, bounded worker-side envelope pass. Streams PCM instead of
/// allocating a second decoded source; larger ordinary timeline samples work too.
pub(crate) fn drag_wave(path:&Path,cancel:&std::sync::atomic::AtomicBool)->Result<Value,String>{
    use std::io::{Read,Seek,SeekFrom};
    use std::sync::atomic::Ordering;
    let mut file=std::io::BufReader::new(std::fs::File::open(path).map_err(|e|e.to_string())?);
    let mut header=[0u8;12];file.read_exact(&mut header).map_err(|e|e.to_string())?;
    if &header[..4]!=b"RIFF"||&header[8..]!=b"WAVE"{return Err("WAV header".into());}
    // Streaming WAV exporters use UINT32_MAX when lengths were not known.
    // Bound those chunks by the actual file, while still rejecting truncated
    // files with explicit lengths.
    let file_end=file.get_ref().metadata().map_err(|e|e.to_string())?.len();
    let declared=u32::from_le_bytes(header[4..8].try_into().unwrap());
    let end=if declared==u32::MAX {file_end}else{declared as u64+8};
    if end>file_end||end<12{return Err("truncated WAV".into());}
    let mut position=12u64;let mut format=None;let mut data=None;
    while position+8<=end {
        file.seek(SeekFrom::Start(position)).map_err(|e|e.to_string())?;
        let mut chunk=[0u8;8];file.read_exact(&mut chunk).map_err(|e|e.to_string())?;
        let declared=u32::from_le_bytes(chunk[4..].try_into().unwrap());
        let size=if &chunk[..4]==b"data"&&declared==u32::MAX {end-position-8}else{declared as u64};
        if position+8+size>end{return Err("WAV chunk".into());}
        if &chunk[..4]==b"fmt " && size>=16 {
            let mut f=[0u8;16];file.read_exact(&mut f).map_err(|e|e.to_string())?;
            let u=|i|u16::from_le_bytes([f[i],f[i+1]]) as usize;
            format=Some((u(0),u(2),u(12),u(14)));
        }else if &chunk[..4]==b"data" {data=Some((position+8,size as usize));}
        position+=8+size+(size%2);
    }
    let (encoding,channels,align,bits)=format.ok_or("WAV format")?;
    let (offset,bytes)=data.ok_or("WAV data")?;
    if !(1..=2).contains(&channels)||!matches!((encoding,bits),(1,8|16|24|32)|(3,32))||align!=channels*(bits/8)||bytes%align!=0||bytes==0{return Err("WAV preview format or budget".into());}
    file.seek(SeekFrom::Start(offset)).map_err(|e|e.to_string())?;
    let frames=bytes/align;let mut wave=Vec::with_capacity(128);let mut buffer=[0u8;16384];
    for bin in 0..128 {
        let mut remaining=((bin+1)*frames/128-bin*frames/128)*align;let mut range=[0f32;2];
        while remaining>0 {
            if cancel.load(Ordering::Relaxed){return Err("cancelled".into());}
            let size=remaining.min(buffer.len()/align*align);file.read_exact(&mut buffer[..size]).map_err(|e|e.to_string())?;
            for sample in buffer[..size].chunks_exact(bits/8){
                let value=match (encoding,bits){
                    (1,8)=>(sample[0] as f32-128.)/128.,
                    (1,16)=>i16::from_le_bytes(sample.try_into().unwrap()) as f32/32768.,
                    (1,24)=>((i32::from_le_bytes([0,sample[0],sample[1],sample[2]]))>>8) as f32/8388608.,
                    (1,32)=>i32::from_le_bytes(sample.try_into().unwrap()) as f32/2147483648.,
                    (3,32)=>f32::from_le_bytes(sample.try_into().unwrap()),_=>unreachable!()
                };
                if !value.is_finite(){return Err("nonfinite WAV".into());}
                range[0]=range[0].min(value.clamp(-1.,1.));range[1]=range[1].max(value.clamp(-1.,1.));
            }
            remaining-=size;
        }
        wave.push(range);
    }
    Ok(json!(wave))
}

#[cfg(test)]mod drag_wave_tests{
 use super::*;use std::sync::atomic::AtomicBool;
 #[test]fn streams_large_sample_envelope_and_cancels(){
  let path=std::env::temp_dir().join(format!("wormwood-drag-wave-{}.wav",std::process::id()));
  let n=9*1024*1024usize;let mut bytes=Vec::with_capacity(n+44);
  bytes.extend_from_slice(b"RIFF");bytes.extend_from_slice(&((n+36)as u32).to_le_bytes());bytes.extend_from_slice(b"WAVEfmt ");bytes.extend_from_slice(&16u32.to_le_bytes());bytes.extend_from_slice(&1u16.to_le_bytes());bytes.extend_from_slice(&1u16.to_le_bytes());bytes.extend_from_slice(&48000u32.to_le_bytes());bytes.extend_from_slice(&48000u32.to_le_bytes());bytes.extend_from_slice(&1u16.to_le_bytes());bytes.extend_from_slice(&8u16.to_le_bytes());bytes.extend_from_slice(b"data");bytes.extend_from_slice(&(n as u32).to_le_bytes());bytes.resize(n+44,128);
  for i in 0..128{bytes[44+i*n/128]=0;bytes[44+(i+1)*n/128-1]=255;}
  std::fs::write(&path,bytes).unwrap();let wave=drag_wave(&path,&AtomicBool::new(false)).unwrap();
  assert_eq!(wave.as_array().unwrap().len(),128);for bin in wave.as_array().unwrap(){assert_eq!(bin,&json!([-1.,127./128.]));}
  assert_eq!(drag_wave(&path,&AtomicBool::new(true)).unwrap_err(),"cancelled");std::fs::remove_file(path).unwrap();
 }
}
