use std::{io::Write,net::TcpStream,path::PathBuf};
fn main(){
 let args:Vec<_>=std::env::args().collect();if args.iter().any(|a|a=="--version"||a=="-version"){println!("2026.10.14");return}
 let value=|name:&str|args.windows(2).find(|a|a[0]==name).map(|a|a[1].clone()).unwrap_or_default();
 let directory=PathBuf::from(value("--out-dir"));let target=value("--progress-target");let mode=std::env::var("FETCH_MIX_FIXTURE_MODE").unwrap_or_default();
 let mut stream:Box<dyn Write>=if target.starts_with("tcp://"){Box::new(TcpStream::connect(target.trim_start_matches("tcp://")).unwrap())}else{#[cfg(unix)]{Box::new(std::os::unix::net::UnixStream::connect(target.trim_start_matches("unix://")).unwrap())}#[cfg(not(unix))]{panic!("expected TCP")}};
 writeln!(stream,"{{\"type\":\"candidate_selected\",\"candidate\":{{\"title\":\"Chosen\",\"url\":\"https://example.invalid/audio\"}}}}").unwrap();
 if mode=="fail"{writeln!(stream,"{{\"type\":\"error\",\"error\":\"native fixture failed\"}}").unwrap();std::process::exit(1)}
 let file=if mode=="outside"{directory.parent().unwrap().join("outside.flac")}else{directory.join("chosen.flac")};
 std::fs::write(&file,if mode=="empty"{&b""[..]}else{&b"audio fixture"[..]}).unwrap();
 let path=file.canonicalize().unwrap();let escaped=path.to_string_lossy().replace('\\',"\\\\").replace('"',"\\\"");
 writeln!(stream,"{{\"type\":\"progress\",\"percent\":37,\"future_field\":\"retained\"}}").unwrap();
 writeln!(stream,"{{\"type\":\"complete\",\"result\":{{\"path\":\"{escaped}\",\"duration_seconds\":121.5,\"bandwidth_hz\":20000,\"bandwidth_rating\":\"full\",\"suggested_gain_db\":-2.5}}}}").unwrap();
 println!("child diagnostic");
}
