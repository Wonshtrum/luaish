#![allow(unused)]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::single_match)]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::match_like_matches_macro)]

use std::io::{Read, Write};

mod ast;
mod editor;
mod interpreter;
mod lexer;
mod logger;
mod parser;
mod resolution;
mod source;

fn main() {
    let mut args = std::env::args();
    if args.len() > 2 {
        panic!("too many arguments");
    }

    let path = args.nth(1);
    let source = if let Some(path) = &path {
        let mut file = std::fs::File::open(path).unwrap();
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        source::Source::new(path.clone(), &content)
    } else {
        source::Source::empty("<empty>".into())
    };

    // let mut lexer = lexer::Lexer::new(&source);
    // let tokens = lexer.tokenize();
    // println!("{tokens:#?}");
    let mut editor = editor::Editor::from_source(source);
    if let Err(error) = editor.run() {
        eprintln!("Editor error: {error}");
    }

    let mut file = std::fs::File::create(path.as_deref().unwrap_or("out.rs")).unwrap();
    let content = editor.ctx.source.content();
    file.write_all(content.as_bytes()).unwrap();
}
