#![allow(dead_code)]

use std::process::{Command, Stdio};
use std::io::Write;
use std::str;

const SHELL_EXE: &'static str = env!("CARGO_BIN_EXE_rust_shell");
const GREEN: &'static str = "\x1b[32m";
const BLUE: &'static str = "\x1b[34m";
const NC: &'static str = "\x1b[0m";

fn trim_debug_output(output: &str) -> (&str, &str) {
    if let Some(pos) = output.find("OUTPUT!!") {
        return (&output[..pos-2], &output[pos+9..]);
    }
    (output, output)
}

fn get_output(cmd: &str) -> String {
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output().unwrap().stdout;
    String::from_utf8(output).unwrap()
}

fn run_test(cmd: &str, expected: String) -> anyhow::Result<()> {
    //spawn the rust shell as a child process
    println!("{}testing{} \"{}\"", BLUE, NC, cmd.trim_end());
    let mut shell = Command::new(SHELL_EXE)
        .arg("--debug")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    {
        let mut shell_stdin = shell.stdin.take().expect("Failed to take shell program stdin");
        shell_stdin.write_all(cmd.as_bytes())?;
    }

    let res = shell.wait_with_output()?;
    assert!(res.status.success());
    let (debug_info, output) = trim_debug_output(str::from_utf8(&res.stdout).unwrap_or(""));
    println!("{}", debug_info);
    assert_eq!(output.trim(), expected.trim());
    println!("{}PASS{}\n", GREEN, NC);
    Ok(())
}

#[test]
fn test_basic() -> anyhow::Result<()> {
    let tests = vec![
        //(<command>, <expected output>)
        ("echo 'hello world'", "hello world".to_string()),
        ("cat Cargo.toml", get_output("cat Cargo.toml")),
        ("ls ..", get_output("ls ..")),
    ];
    for test in tests.into_iter() {
        run_test(test.0, test.1)?;
    }
    Ok(())
}

#[test]
fn test_builtins() -> anyhow::Result<()> {
    //to test history, set history file to temp file. then run commands below, then run history and
    //match output
    let cwd = std::env::current_dir().unwrap();
    let parent_dir = cwd.parent().unwrap();
    let tests = vec![
        ("pwd", format!("{}", cwd.display())),
        ("cd ../ && pwd", format!("{}", parent_dir.display())),
        ("cd ~/&&pwd", format!("{}",std::env::home_dir().unwrap().display())),
    ];
    for test in tests.into_iter() {
        run_test(test.0, test.1)?;
    }
    Ok(())
}

#[test]
fn test_pipeline() -> anyhow::Result<()> {
    let tests = vec![
        // Basic 2-stage pipeline
        ("cat Cargo.toml | grep dependencies", get_output("cat Cargo.toml | grep dependencies")),
        // The classic 3-stage pipeline (Tests for File Descriptor leaks)
        ("echo 'apple banana cherry' | tr ' ' '\\n' | grep a\n", "apple\nbanana".to_string()),
        // Counting lines (verifies EOF propagation so `wc` doesn't hang)
        //("echo -e 'line1\\nline2\\nline3' | wc -l\n", "3".to_string()), <- this doesn't work
        //because bin echo doesn't know how to parse the -e flag
        ("printf 'line1\\nline2\\nline3\\n' | wc -l\n", "3".to_string()),
        // Builtins piping to external commands
        ("echo 'reverse me' | rev\n", "em esrever".to_string()),
        // Exit status propagation (false fails, but echo succeeds)
        ("false | echo 'survived'\n", "survived".to_string()),
        //  Large output buffering (prevents OS pipe buffer deadlocks)
        ("seq 1 1000 | head -n 3\n", "1\n2\n3".to_string()),
    ];
    for test in tests.into_iter() {
        run_test(test.0, test.1)?;
    }
    Ok(())
}

#[test]
fn test_heredocs() -> anyhow::Result<()> {
    let heredoc_tests = vec![
        // Basic Heredoc
        (
            "cat << EOF\nhello\nworld\nEOF\n", 
            "hello\nworld".to_string()
        ),
        // Empty Heredoc: Verifies the shell handles an immediate delimiter cleanly without crashing.
        (
            "cat << EOF\nEOF\n", 
            "".to_string()
        ),
        // Preservation of Whitespace/Indentation: Heredocs must preserve leading spaces inside the body.
        (
            "cat << EOF\n  nested line\n    deeply nested line\nEOF\n", 
            "  nested line\n    deeply nested line".to_string()
        ),
        // Heredoc Piped into a Filter: Verifies that the heredoc contents are fed into the pipeline chain correctly.
        (
            "cat << EOF | grep target\nignore this line\nthis is the target\nskip this too\nEOF\n", 
            "this is the target".to_string()
        ),
        // Heredoc Piped into a Counter: Verifies EOF closure so tools like wc don't hang indefinitely.
        (
            "cat << EOF | wc -w\nrust language shell execution\nEOF\n", 
            "4".to_string()
        ),
        // Nested Quotes Inside Heredoc Body: The body of a standard heredoc treats quotes as literal characters, not syntax.
        (
            "cat << EOF\necho \"hello\"\nprint('world')\nEOF\n", 
            "echo \"hello\"\nprint('world')".to_string()
        ),
        //multi heredoc
        (
            "cat << A << B << C\nFirst\nA\nSecond\nB\nThird\nC\n",
            "First\nSecond\nThird".to_string()
        ),
        //no spaces between operator and delimiter
        (
            "cat <<eof\nThis should work!\neof\n", 
            "This should work!".to_string()
        ),
    ];
    for test in heredoc_tests.into_iter() {
        run_test(test.0, test.1)?;
    }
    Ok(())
}

#[test]
fn test_redirects() -> anyhow::Result<()> {
    let mut filecontent = "binturong bin\nbinder\nbingchilling";
    let mut tests: Vec<(&str, String)> = vec![
        ("echo \"binturong bin\nbinder\nbingchilling\" > temp.txt", "".to_string()),
        ("<temp.txt cat\n", filecontent.to_string()),
        ("grep 'binturong' < temp.txt\n", "binturong bin".to_string()),
        ("wc -l < temp.txt\n", "3".to_string()),
        ("cat   <     temp.txt    \n", filecontent.to_string()),
    ];
    for test in tests.into_iter() {
        run_test(test.0, test.1)?;
    }
    filecontent = "binturong bin\nbinder\nbingchilling\nappended content";
    tests = vec![
        //redirect operator can be anywhere in command
        (">> temp.txt echo 'appended content'\n", "".to_string()),
        ("<temp.txt cat\n", filecontent.to_string()),
        //multidirection redirect
        ("< temp.txt >> temp2.txt cat \n", "".to_string()),
        ("cat < temp2.txt\n", filecontent.to_string()),
        (">temp2.txt cat << EOF\nwowzers\nEOF\n", "".to_string()),
        ("cat <temp2.txt\n", "wowzers".to_string()),
        //multiple redirect in
        ("echo 'binturong' > temp.txt\n", "".to_string()),
        ("cat < temp.txt < temp.txt\n", "binturong\nbinturong".to_string()),
        ("cat <temp.txt <<EOF\nbingchilling\nEOF\n", "binturong\nbingchilling".to_string()),
        //multiple redirect out
        ("echo 'duplicated' > temp.txt >temp2.txt\n", "".to_string()),
        ("cat <temp.txt < temp2.txt\n", "duplicated\nduplicated".to_string()),
        // all 4
        ("<<EOF < temp.txt cat >> temp2.txt > temp3.txt\nbinturong\nEOF\n", "".to_string()),
        ("cat < temp2.txt\n", "duplicated\nbinturong\nduplicated".to_string()),
        ("cat < temp3.txt\n", "binturong\nduplicated".to_string()),
    ];
    for test in tests.into_iter() {
        run_test(test.0, test.1)?;
    }
    // cleanup
    Command::new("rm").arg("temp.txt").status()?;
    Command::new("rm").arg("temp2.txt").status()?;
    Command::new("rm").arg("temp3.txt").status()?;
    Ok(())
}

        // // Combining Heredocs and Pipelines
        // ("cat << EOF | grep 'match'\nignore\nmatch this\nignore again\nEOF\n", "match this".to_string()),
