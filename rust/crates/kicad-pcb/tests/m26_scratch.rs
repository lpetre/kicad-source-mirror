// Scratch differential run (package M26 development); replaced by m26_generated_tables.rs.
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board_report};

fn norm(text: &str, input: &str) -> String {
    let mut out = String::new();
    let mut n = 0;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("(uuid \"") && !input.contains(t) {
            n += 1;
            out.push_str(&format!("{}(uuid NEW{})\n", &line[..line.len() - t.len()], n));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[test]
#[ignore]
fn scratch() {
    let dir = std::path::Path::new("/tmp/claude-0/m26");
    let mut names: Vec<_> = std::fs::read_dir(dir.join("in")).unwrap().map(|e| e.unwrap().path()).collect();
    names.sort();
    for p in names {
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        let input = std::fs::read_to_string(&p).unwrap();
        let reference = std::fs::read_to_string(dir.join("ref").join(&name)).unwrap();
        let (mut board, _) = parse_board_report(&input, &name, &ParseOptions::default()).unwrap();
        let n = kicad_pcb::board_tables::refresh_generated_tables(&mut board);
        let out = format_board(&board, &FormatOptions::default()).unwrap();
        let (a, b) = (norm(&out, &input), norm(&reference, &input));
        std::fs::create_dir_all(dir.join("rs")).unwrap();
        std::fs::write(dir.join("rs").join(&name), &a).unwrap();
        std::fs::write(dir.join("rs").join(format!("{name}.ref")), &b).unwrap();
        println!("{name}: refreshed {n}, identical={}", a == b);
    }
}
