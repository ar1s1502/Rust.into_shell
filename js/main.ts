import { Terminal } from '@xterm/xterm';
import { invoke, Channel } from '@tauri-apps/api/core';
import { add_prompt_continuation, clear } from './cl.ts';

//history: array of Strings, size N. add every input to the history at history[ptr % N], then N++


/* SETUP */
//must match with ../src/shell.rs OSC data const's
const PROMPT_NORMAL = "A" as const;
const PROMPT_CONTINUE = "E" as const;
const PROMPT_MISSING = "F" as const;

const term = new Terminal({
    rows: 30,
    cols: 160,
    fontSize: 11,
});
term.open(document.getElementById('xterm') as HTMLDivElement);

const decoder = new TextDecoder('utf-8');
const OSC133_REGEX = /\x1b]133;([^\x07]+)\x07/;
const pty_channel = new Channel<Uint8Array>();
pty_channel.onmessage = (msg_) => {
    //rust_shell will send OSC133 sequences to indicate some shell state
    const msg = decoder.decode(new Uint8Array(msg_));
    const result = msg.match(OSC133_REGEX);
    if (result) {
        console.log(`result: ${result}`);
        const data = result[1].split(";"); //split capture group (OSC sequence data) by ;
        console.log(`data: ${data}`);
        switch (data[0]) {
            case PROMPT_NORMAL:
                //ignore regular prompts because they're handled by the cl textarea
                return;
            case PROMPT_CONTINUE:
                add_prompt_continuation(data[1]);
                return;
            case PROMPT_MISSING:
                console.log("missing prompt!")
                add_prompt_continuation(data[1]);
                return;
            default:
                //unrecognized OSC sequence, error
                return;
        }
    }

    //if not an OSC133 sequence, then must be regular data
    //display to xterm
    term.write(msg);
};

await invoke('pty_read', {
    ptyChannel: pty_channel
}).then(() => {
    console.log("pty_read pipe setup success")
});
    //TODO CATCH ERROR;
/* END SETUP */

/* Submit to PTY logic */
function writeToPty(input: string) {
    invoke('pty_write', {
        cliInput: input
    });//TODO: catch errors
}

const input = document.getElementById("cl") as HTMLTextAreaElement;
let suggestions = document.getElementById("suggestions") as HTMLUListElement;

input.addEventListener('keydown', (event) => {
    if (event.key === "Enter" && !event.shiftKey) {
        event.preventDefault(); //stops the newline from being added after this block finishes execution

        const payload = input.value + "\n";
        console.log(`submitting ${payload}`);
        writeToPty(payload);

        //TODO add input.value.trim() to history

        input.value = "";
        clear(suggestions);
    }
});

