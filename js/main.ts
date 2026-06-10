import { Terminal } from '@xterm/xterm';
import { invoke, Channel } from '@tauri-apps/api/core';
import { listen, } from '@tauri-apps/api/event';
import { add_prompt_continuation, get_cur_cmd, reset_cl } from './cl.ts';
import { add_to_hist } from './history.ts';

const term = new Terminal({
    rows: 30,
    cols: 160,
    fontSize: 11,
});
term.open(document.getElementById('xterm') as HTMLDivElement);

const pty_channel = new Channel<Uint8Array>();
pty_channel.onmessage = (msg_) => {
    handle_output(new Uint8Array(msg_));
}

await invoke('pty_read', {
    ptyChannel: pty_channel
}).then(() => {
    console.log("pty_read pipe setup success")
}); 

listen<string>('prompt_continue', (event) => {
    console.log(event.payload);
    add_prompt_continuation(event.payload);
});

listen<null>('output_start', (_) => {
    const cmd = get_cur_cmd().trim();
    console.log(cmd);
    term.writeln(`output for ${cmd.replaceAll("\n", "\r\n")}:`);        
    add_to_hist(cmd);        
    reset_cl();
});


function handle_output(output: Uint8Array) {
    term.write(output);
}

