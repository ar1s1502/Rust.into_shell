TODO: 
  - pseudo terminal (PTY)
  - tilde expansion logic, system user display
  - tauri/react gui integration
  - ||, &&, and & support
  - autocomplete for shell commands

features so far:
  - unix CLI tools supported via searching through $PATH variable
  - piping (e.g. cat Cargo.toml | head -n 10 | grep "bin")
  - redirection (<, >, >>)
  - heredoc support (<< operator)
  - editor history with [Rustyline](https://docs.rs/rustyline/18.0.0/rustyline/) DefaultEditor, arrow key support
  - Fully custom, state-aware, error-aware shell Lexer using the [Logos](https://docs.rs/logos/latest/logos/) crate. will pretty much behave exactly like Bash/zsh. "state" is managed through Logos as well. Different Logos enums = different shell lexer state; for example the normal lexer, which matches words and shell operators, will switch into a different lexer upon encountering a quote token like \` ' or " (for example a `>` is interpreted literally if within quotes, while it is the stdout redirect operator otherwise)

**USE**
if have cargo/rust, do `cargo r` from terminal within project directory. if not, wait till i make this a desktop app lol\n

Cool commands to try

 ```
cat << EOF | grep "bin" >> tmp.txt
 bingchilling
 binturong
 EOF
```
 should append
 ```
 bingchilling
 binturong
```
 to tmp.txt

     ```
     cat << one << two << three
     FIRST
     one
     SECOND
     two
     THIRD
     three
     ```
     should print
```
FIRST
SECOND
THIRD
```

