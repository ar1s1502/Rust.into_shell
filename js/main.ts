import { IDisposable, Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { invoke, Channel } from '@tauri-apps/api/core';
import { listen, } from '@tauri-apps/api/event';
import { add_prompt_continuation, get_cur_cmd, reset_cl, add_hist_li, writeToPty } from './cl.ts';
import { add_to_hist, } from './history.ts';

const term = new Terminal({
    rows: 30,
    cols: 160,
    fontSize: 11,
});
term.open(document.getElementById('xterm') as HTMLDivElement);
const fit = new FitAddon();
term.loadAddon(fit);
fit.fit();
await invoke('resize_pty', {
    cols: term.cols,
    rows: term.rows,
}).catch((err) => {
    console.log('failed to resize pty');
    console.log(`tauri err: ${err}`);
}).finally(() => {
    console.log(term.cols, term.rows);
});

//Fullscreen app support
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
})

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
    document.getElementById('history_ul')?.lastElementChild?.scrollIntoView({ behavior: 'smooth', block: 'end'});
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

// resizing
const resizer = document.getElementById('drag_bar') as HTMLDivElement;
const top_panel = document.getElementById('top_panel') as HTMLDivElement;
resizer.addEventListener('mousedown', ()=> {
    document.addEventListener('mousemove', resize);
    document.addEventListener('mouseup', stop_resize);
    document.body.classList.add('select-none');
});

function debounce_fit(time: number) {
    let timer: number;
    return function() {
        clearTimeout(timer);

        timer = setTimeout(async ()=> {
            fit.fit();
            await invoke('resize_pty', {
                cols: term.cols,
                rows: term.rows,
            }).catch((err) => {
                console.log('failed to resize pty');
                console.log(`tauri err: ${err}`);
            }).finally(()=> {
                console.log(term.cols, term.rows);
            });
        }, time);
    }
}

function resize(event: MouseEvent) {
    let start_y = event.clientY;
    top_panel.style.height = `${start_y}px`;
    const resize_backend_pty = debounce_fit(350); //wait until 350 ms have passed after stop_resize called, then resize
    resize_backend_pty();
}

async function stop_resize(_: MouseEvent) {
    document.removeEventListener('mousemove', resize);
    document.body.classList.remove('select-none');
    document.removeEventListener('mouseup', stop_resize);
}
