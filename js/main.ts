import { Terminal } from '@xterm/xterm';
import { invoke, Channel } from '@tauri-apps/api/core';
import { add_prompt_continuation, clear, get_cur_cmd, reset_cl } from './cl.ts';

//history: array of Strings, size N. add every input to the history at history[ptr % N], then N++



//OSC133 sequences
//see https://contour-terminal.org/vt-extensions/osc-133-shell-integration/
//must match with ../src/shell.rs OSC data const's
const PROMPT_START = "A" as const;
const PROMPT_CONTINUE = "pc" as const;
const PROMPT_END = "B" as const;
const CMD_OUTPUT_START = "C" as const;
const CMD_END = "D" as const;
const SYN_ERR = "syn_err" as const;

let SHELL_MODE: "A" | "B" | "C" | "D" = CMD_END;

const term = new Terminal({
    rows: 30,
    cols: 160,
    fontSize: 11,
});
term.open(document.getElementById('xterm') as HTMLDivElement);


let stream_buf = new Uint8Array(0);
const decoder = new TextDecoder('utf-8');
const pty_channel = new Channel<Uint8Array>();
pty_channel.onmessage = (msg_) => {
    //rust_shell will send OSC133 sequences to indicate some shell state
    //format: \x1b]133;${data}\x07

    //merge chunk of bytes with stream buffer
    let bytes = new Uint8Array(msg_);
    let extended = new Uint8Array(stream_buf.length + bytes.length);
    extended.set(stream_buf, 0);
    extended.set(bytes, stream_buf.length);
    stream_buf = extended;

    //find the start of the OSC133 sequence (\x1b]), if any
    let seq_start = 0;
    while (seq_start < stream_buf.length) {
        if (stream_buf[seq_start] === 0x1b) {
            if (seq_start + 1 >= stream_buf.length) {
                break; //out of bytes, therefore osc seq is cut off
            }
            if (stream_buf[seq_start + 1] !== 0x5D) {
                seq_start++;
                continue; //not an osc sequence, treat as normal bytes for xterm
            }

            let seq_end = -1;
            for (let i = seq_start + 2; i < stream_buf.length; i++) {
                if (stream_buf[i] === 0x07) {
                    seq_end = i;
                    break;
                }
            }  
            if (seq_end !== -1) {
                //found complete osc sequence
                //output any data before the osc seq
                handle_output(stream_buf.subarray(0, seq_start));
                //set shell mode
                const data = decoder.decode(stream_buf.subarray(seq_start + 2, seq_end)); // 133;${codes}
                parse_osc_data(data);

                stream_buf = stream_buf.subarray(seq_end + 1);
                seq_start = 0;
                continue;
            } else { //osc seq was cut off
                break; //wait for next onmessage event
            }
            
        }
        seq_start++;
    }
    if (seq_start === stream_buf.length && stream_buf.length > 0) {
        //no osc sequence in remaining bytes
        handle_output(stream_buf);
        stream_buf = new Uint8Array(0);
    }
};

await invoke('pty_read', {
    ptyChannel: pty_channel
}).then(() => {
    console.log("pty_read pipe setup success")
});
/* END SETUP */

function parse_osc_data(data: string) {
    if (data.length === 0 || !data.startsWith('133')) { return; }
    let data_arr = data.split(';');
    switch (data_arr[1]) {
        case PROMPT_START:
            SHELL_MODE = PROMPT_START;
            if (data_arr[2] === PROMPT_CONTINUE) {
                if (data_arr.length >= 4) {
                    add_prompt_continuation(data_arr[3]);
                } else {
                    console.log("ERR: invalid prompt continuation format")
                }
            }
            break;
        case CMD_OUTPUT_START:
            if (SHELL_MODE !== PROMPT_END && SHELL_MODE !== CMD_END) {
                console.log("ERR: invalid shell state transition");
                return;
            }
            SHELL_MODE = CMD_OUTPUT_START;
            break;
        case PROMPT_END:
            if (SHELL_MODE !== PROMPT_START) {
                console.log("ERR: invalid shell state transition");
                return;
            }
            SHELL_MODE = PROMPT_END;

            const cmd = get_cur_cmd().trim();
            term.writeln(`output for ${cmd}:`);
            //TODO: add prompt to history

            reset_cl();

            break;
        case CMD_END:
            if (SHELL_MODE !== CMD_OUTPUT_START) {
                console.log("ERR: invalid shell state transition");
                return;
            }
            SHELL_MODE = CMD_END;
            break;
        default:
            console.log("ERR: not a valid osc 133 sequence");
    }
}

function handle_output(output: Uint8Array) {
    if (output.length === 0) { return; }
    if (SHELL_MODE === CMD_OUTPUT_START) {
        console.log("output: " + decoder.decode(output));
        //display to xterm
        term.write(output);
    }
}

