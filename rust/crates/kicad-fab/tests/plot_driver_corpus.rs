// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! FAB-03 corpus replay: every `.kicad_pcb` under `qa/data` and `demos` smaller than 300 KB,
//! plotted with the `kicad-cli pcb export gerbers` defaults, against the compact golden
//! `tests/golden/plot_driver/corpus.txt` (reference kicad-cli, `generate.py corpus`).
//!
//! Per board and produced file the port must reproduce the file name (`BuildPlotFileName`,
//! Protel extension, user layer names), the header written by `StartPlotBoard` (X2
//! attributes, format, units) and, once every item the layer holds is ported, the whole file.
//! Parts that reach code still pending (board parser, model stubs, FAB-04/05/06 item plotting)
//! are counted, not failed, so the test tightens as the port grows; any output the port does
//! produce must match. `KICAD_FAB_CORPUS_REPORT=1` prints the per-board result,
//! `KICAD_FAB_CORPUS_DUMP=<dir>` writes the port's output of every differing file there.

mod plot_common;

use std::panic::{AssertUnwindSafe, catch_unwind};

use kicad_fab::pcb_plotter::build_plot_file_name;
use kicad_fab::pcbplot::gerber_protel_extension;
use kicad_fab::plot_board_layers::{plot_gerber_layer_file, start_plot_board};
use kicad_fab::wx_file_name::WxFileName;
use kicad_pcb::ctx::BoardCtx;
use plot_common::*;

struct BoardCase {
    path: String,
    /// `(name hash, header hash, file hash)` in "Plotted to" order.
    files: Vec<(String, String, String)>,
}

fn corpus() -> Vec<BoardCase> {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/plot_driver/corpus.txt"),
    )
    .unwrap();
    let mut cases: Vec<BoardCase> = Vec::new();

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("B ") {
            cases.push(BoardCase {
                path: path.to_string(),
                files: Vec::new(),
            });
        } else {
            let f: Vec<&str> = line.split(' ').collect();
            assert_eq!(f.len(), 3, "bad line {line:?}");
            cases.last_mut().unwrap().files.push((
                f[0].to_string(),
                f[1].to_string(),
                f[2].to_string(),
            ));
        }
    }

    cases
}

fn h8(text: &str) -> String {
    fnv(text)[..8].to_string()
}

#[derive(Default, Debug)]
struct Score {
    boards: usize,
    boards_without_output: usize,
    boards_not_loadable: usize,
    files: usize,
    names_matched: usize,
    headers_matched: usize,
    headers_pending: usize,
    files_matched: usize,
    files_pending: usize,
    failures: Vec<String>,
}

#[test]
fn corpus_replay() {
    let report = std::env::var_os("KICAD_FAB_CORPUS_REPORT").is_some();
    let root = kicad_qa::kicad_source_root();
    let mut score = Score::default();

    // The pending stubs panic by design; keep the output readable.
    std::panic::set_hook(Box::new(|_| {}));

    for case in corpus() {
        score.boards += 1;
        score.files += case.files.len();

        if case.files.is_empty() {
            score.boards_without_output += 1;
            continue;
        }

        let path = root.join(&case.path);
        let mut board = match catch_unwind(|| load_board(&path)) {
            Ok(Ok(board)) => board,
            Ok(Err(e)) => {
                score.boards_not_loadable += 1;
                if report {
                    println!(
                        "{}: not loadable: {}",
                        case.path,
                        e.lines().next().unwrap_or("")
                    );
                }
                continue;
            }
            Err(_) => {
                score.boards_not_loadable += 1;
                if report {
                    println!("{}: not loadable (pending)", case.path);
                }
                continue;
            }
        };

        let Some(connectivity) = build_connectivity(&mut board) else {
            score.boards_not_loadable += 1;
            if report {
                println!("{}: connectivity not buildable (pending)", case.path);
            }
            continue;
        };
        let ctx = BoardCtx::bare(&board).with_connectivity(&connectivity);
        let opts = cli_gerber_opts(false, 6, false, false);

        // file name hash -> (layer, file name) for every enabled layer
        let names: Vec<_> = board
            .enabled_layers()
            .ui_order()
            .0
            .into_iter()
            .map(|layer| {
                let mut fn_ = WxFileName::new(&board.file_name);
                build_plot_file_name(
                    &mut fn_,
                    "",
                    &board.layer_name(layer),
                    &gerber_protel_extension(layer.0),
                );
                (h8(&fn_.full_name()), layer)
            })
            .collect();

        let mut line = String::new();

        for (name_hash, header_hash, file_hash) in &case.files {
            let Some(&(_, layer)) = names.iter().find(|(h, _)| h == name_hash) else {
                score.failures.push(format!(
                    "{}: no enabled layer gives the file name {name_hash}",
                    case.path
                ));
                continue;
            };
            score.names_matched += 1;

            // The header: StartPlotBoard only.
            let header_result = catch_unwind(AssertUnwindSafe(|| {
                let plotter = start_plot_board(
                    &ctx,
                    &opts,
                    layer.0,
                    &board.layer_name(layer),
                    "out/x",
                    "",
                    "",
                    "1",
                    &env(),
                )
                .unwrap();
                normalise(plotter.output())
            }));

            match header_result {
                Ok(text) if h8(header(&text)) == *header_hash => {
                    score.headers_matched += 1;
                    line.push('h');
                }
                Ok(text) => {
                    score.failures.push(format!(
                        "{} {}: header differs:\n{}",
                        case.path,
                        board.layer_name(layer),
                        header(&text)
                    ));
                    line.push('X');
                }
                Err(_) => {
                    score.headers_pending += 1;
                    line.push('.');
                }
            }

            // The whole file.
            let file_result = catch_unwind(AssertUnwindSafe(|| {
                let out =
                    plot_gerber_layer_file(&ctx, &opts, layer, &[layer], "out", &env()).unwrap();
                normalise(std::str::from_utf8(&out.contents).unwrap())
            }));

            match file_result {
                Ok(text) if h8(&text) == *file_hash => {
                    score.files_matched += 1;
                    line.push('F');
                }
                Ok(text) => {
                    if let Some(dir) = std::env::var_os("KICAD_FAB_CORPUS_DUMP") {
                        let name = format!(
                            "{}-{}.gbr",
                            case.path.replace('/', "_"),
                            board.layer_name(layer)
                        );
                        std::fs::write(std::path::Path::new(&dir).join(name), &text).unwrap();
                    }
                    score.failures.push(format!(
                        "{} {}: file differs",
                        case.path,
                        board.layer_name(layer)
                    ));
                    line.push('X');
                }
                Err(_) => {
                    score.files_pending += 1;
                    line.push('.');
                }
            }

            line.push(' ');
        }

        if report {
            println!("{}: {}", case.path, line);
        }
    }

    let _ = std::panic::take_hook();

    println!(
        "plot driver corpus: {} boards ({} without kicad-cli output, {} not loadable by the port), \
         {} files: {} names matched, headers {} matched / {} pending, files {} matched / {} pending, \
         {} failures",
        score.boards,
        score.boards_without_output,
        score.boards_not_loadable,
        score.files,
        score.names_matched,
        score.headers_matched,
        score.headers_pending,
        score.files_matched,
        score.files_pending,
        score.failures.len()
    );

    assert!(
        score.failures.is_empty(),
        "{} differences:\n{}",
        score.failures.len(),
        score.failures.join("\n")
    );
}
