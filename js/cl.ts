import { invoke, } from '@tauri-apps/api/core';
import { fuzzy_search } from './history';

const PASTE_START = "\x1b[200~";
const PASTE_END = "\x1b[201~";

/* Utilities */
export function clear(ele: HTMLElement) {
    ele.innerHTML = "";
    ele.style.display = "none";
}

export function display(ele: HTMLElement) {
    ele.style.display = "block";
}

export function make_active(ele: HTMLTextAreaElement) {
    ele.readOnly = false;
    ele.classList.add("focus:border-teal-500");
    active_input = ele;
    ele.addEventListener('keydown', submit_listener);
    ele.addEventListener('keydown', interrupt_listener);
    ele.addEventListener('paste', paste_listener);
    ele.oninput = searchbar_handler;
    ele.focus();
}

export function make_inactive(ele: HTMLTextAreaElement) {
    ele.readOnly = true;
    ele.classList.remove("focus:border-teal-500");
    ele.removeEventListener('keydown', searchbar_handler);
    ele.removeEventListener('keydown', interrupt_listener);
    ele.removeEventListener('paste', paste_listener);
    ele.oninput = null;
    //TODO: gray out text? or make it more in the bg somehow
}

export function get_cur_cmd() {
    return cur_cmd;
}

export function reset_cl() {
    cur_cmd = "";
    cmd_line.value = "";
    make_active(cmd_line);
    clear(continuation_prompts);
}
/* *** */

const cmd_line = document.getElementById("cl") as HTMLTextAreaElement;
let active_input: HTMLTextAreaElement = cmd_line;
const suggestions = document.getElementById("suggestions") as HTMLUListElement;
let cur_cmd = ""; //the current user input command (from all the input textareas)
make_active(cmd_line);

function searchbar_handler() {
    // Check if the current OS webview engine supports the modern CSS
    // if (!CSS.supports('field-sizing', 'content')) {
    //     // Reset height to recalculate, then set it exactly to the scroll height
    //     active_input.style.height = 'auto';
    //     active_input.style.height = active_input.scrollHeight + 'px';
    // }
    clear(suggestions);
    if (active_input.value.trim() === "") return; 
    let matches = fuzzy_search(active_input.value);
    if (matches) display(suggestions); else return;
    for (const match of matches) {
        const li = document.createElement('li');
        li.textContent = match.item;
        li.onclick = ()=> {
            active_input.value = match.item;
            cur_cmd = "";
            active_input.rows = 1 + (match.item.match(/\n/g) || []).length;
            clear(suggestions);
            active_input.focus();
        }
        li?.classList.add("px-1", "py-0", "m-1", "cursor-pointer", "hover:text-white", "hover:bg-gray-800", "transition-colors");
        suggestions.appendChild(li);
    }
    // resize the textarea to accommodate any new lines
    active_input.rows = 1 + (active_input.value.match(/\n/g) || []).length;
}

function writeToPty(input: string) {
    invoke('pty_write', {
        cliInput: input
    });//TODO: catch errors
}

function submit_listener(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey) {
        event.preventDefault(); //stops the newline from being added after this block finishes execution

        let input = (active_input.value).replaceAll(/\r/g, ""); //strip all carriage returns
        if (input.includes('\n')) {
            input = `${PASTE_START}${input}${PASTE_END}`; //must simulate bracketed paste, 
            // otherwise rustyline ignores everything after 1st newline char
        }
        input += '\n';
        cur_cmd += input;
        console.log(`submitting ${cur_cmd}`);
        writeToPty(input);

        clear(suggestions);
    }
}

function interrupt_listener(event: KeyboardEvent) {
    if (event.ctrlKey) {
        const keypress = event.key.toLowerCase();
        if (keypress === 'c') {
            reset_cl();
            writeToPty("\x03");
        } else if (keypress === 'd') {
            reset_cl();
            writeToPty("\x04");
        }
    }
}

function paste_listener(event: ClipboardEvent) {
    if (event.clipboardData) {
        const paste_text = event.clipboardData.getData('text/plain');
        const num_new_lines = (paste_text.match(/\n/g) || []).length;
        active_input.rows += num_new_lines;
    }
}

const continuation_prompts = document.getElementById("continuation_prompts") as HTMLUListElement;
export function add_prompt_continuation(prompt_text: string) {
    make_inactive(active_input);
    const textarea = document.createElement('textarea');
    textarea.rows = 1;
    textarea.spellcheck=false;
    textarea.autocapitalize="off";
    textarea.autocomplete="off";
    textarea.setAttribute("autocorrect", "off");
    textarea.setAttribute("spellcheck", "false");
    textarea.setAttribute("data-gramm", "false");
    textarea.setAttribute("field-sizing", "content");
    textarea?.classList.add("p-2", "bg-gray-800", "rounded", "outline-none", "border-2", "inline-block",
        "border-transparent", "hover:border-slate-500", "transition-colors", "flex-grow");
    
    const prompt = document.createElement('p');
    prompt.textContent = prompt_text;
    prompt.classList.add("text-zinc-500", "select-none", "inline-block", "ml-3" )

    const li = document.createElement('li');
    //make the li have {prompt} {textarea} side by side
    li.classList.add("flex", "flex-row", "w-full", "mb-1", "items-center", "justify-start");
    li.appendChild(prompt);
    li.appendChild(textarea);

    continuation_prompts.appendChild(li);
    display(continuation_prompts);
    make_active(textarea);
}