import Fuse from 'fuse.js' //tokenize by word + fuzzy search using bitap algo
import { invoke, } from '@tauri-apps/api/core';


const items = [
    "as;fhas", "askjdfja", "llll", "uioiuio\nadf", "hjkjhjk", "bnmnbnbv", "trtytty", "Decadence\ndfd", "Tones", "Octet", "CJC", "dts", "air"
];
const fuse = new Fuse(items, {
    includeMatches: true,
    useTokenSearch: true,
});

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
    ele.oninput = searchbarHandler;
    ele.focus();
}

export function make_inactive(ele: HTMLTextAreaElement) {
    ele.readOnly = true;
    ele.classList.remove("focus:border-teal-500");
    ele.removeEventListener('keydown', searchbarHandler);
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
cmd_line.oninput = searchbarHandler;
cmd_line.addEventListener('keydown', submit_listener);
let active_input: HTMLTextAreaElement = cmd_line;
const suggestions = document.getElementById("suggestions") as HTMLUListElement;
let cur_cmd = ""; //the current user input command (from all the input textareas)

function searchbarHandler() {
    // Check if the current OS webview engine supports the modern CSS
    if (!CSS.supports('field-sizing', 'content')) {
        // Reset height to recalculate, then set it exactly to the scroll height
        active_input.style.height = 'auto';
        active_input.style.height = active_input.scrollHeight + 'px';
    }
    clear(suggestions);
    if (active_input.value.trim() === "") return; 
    let matches = fuse.search(active_input.value);
    if (matches.length != 0) display(suggestions);
    for (const match of matches) {
        const li = document.createElement('li');
        li.textContent = match.item;
        li.onclick = ()=> {
            active_input.value = li.textContent;
            clear(suggestions);
        }
        li?.classList.add("px-1", "py-0", "m-1", "cursor-pointer", "hover:text-white", "hover:bg-gray-800", "transition-colors");
        suggestions.appendChild(li);
    }
}

function writeToPty(input: string) {
    invoke('pty_write', {
        cliInput: input
    });//TODO: catch errors
}

function submit_listener(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey) {
        event.preventDefault(); //stops the newline from being added after this block finishes execution

        const input = active_input.value + "\n";
        cur_cmd += input;
        console.log(`submitting ${cur_cmd}`);
        writeToPty(input);

        clear(suggestions);
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
    textarea.setAttribute("data-gramm", "false");
    textarea?.classList.add("p-2", "bg-gray-800", "rounded", "outline-none", "border-2", "inline-block",
        "border-transparent", "hover:border-slate-500", "transition-colors");
    
    const prompt = document.createElement('p');
    prompt.textContent = prompt_text;
    prompt.classList.add("text-zinc-500", "select-none", "inline-block", )

    const li = document.createElement('li');
    //make the li have {prompt} {textarea} side by side
    li.classList.add("block", "w-full", "mb-1", "clear-both");
    li.appendChild(prompt);
    li.appendChild(textarea);

    continuation_prompts.appendChild(li);
    display(continuation_prompts);
    make_active(textarea);
}