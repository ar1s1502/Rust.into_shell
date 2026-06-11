import { IDisposable, Terminal } from '@xterm/xterm';
import { invoke, Channel } from '@tauri-apps/api/core';
import { listen, } from '@tauri-apps/api/event';
import { add_prompt_continuation, get_cur_cmd, reset_cl, add_hist_li } from './cl.ts';
import { add_to_hist, } from './history.ts';

const term = new Terminal({
    rows: 30,
    cols: 160,
    fontSize: 11,
});
term.open(document.getElementById('xterm') as HTMLDivElement);

let is_fullscreen = false;
let stdin_hook: IDisposable | null = null;

const pty_channel = new Channel<Uint8Array>();
pty_channel.onmessage = (msg_) => {
    handle_output(new Uint8Array(msg_));
}

function handle_output(output: Uint8Array) {
    term.write(output);
}

await invoke('pty_read', {
    ptyChannel: pty_channel
}).then(() => {
    console.log("pty_read pipe setup success")
}); 

export function writeToPty(input: string) {
    invoke('pty_write', {
        cliInput: input
    });
}

listen<string>('prompt_continue', (event) => {
    console.log(event.payload);
    add_prompt_continuation(event.payload);
});

listen<null>('output_start', (_) => {
    const cmd = get_cur_cmd().trim();
    console.log(cmd);
    term.writeln(`output for ${cmd.replaceAll("\n", "\r\n")}:`);        
    add_to_hist(cmd);        
    add_hist_li(cmd);
    reset_cl("");
});

listen<null>('enter_fullscreen', (_) => {
    is_fullscreen = true;
    stdin_hook = term.onData(data => {
        writeToPty(data);
    });
});

listen<null>('exit_fullscreen', (_) => {
    is_fullscreen = false;
    stdin_hook?.dispose();
});



