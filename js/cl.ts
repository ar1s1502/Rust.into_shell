import Fuse from 'fuse.js' //tokenize by word + fuzzy search using bitap algo
import { invoke, } from '@tauri-apps/api/core';


const items = [
    "as;fhas", "askjdfja", "llll", "uioiuio\nadf", "hjkjhjk", "bnmnbnbv", "trtytty", "Decadence\ndfd", "Tones", "Octet", "CJC", "dts", "air"
];
const fuse = new Fuse(items, {
    includeMatches: true,
    useTokenSearch: true,
});

export function clear(ele: HTMLElement) {
    ele.innerHTML = "";
    ele.style.display = "none";
}

export function display(ele: HTMLElement) {
    ele.style.display = "block";
}

const input = document.getElementById("cl") as HTMLTextAreaElement;
const suggestions = document.getElementById("suggestions") as HTMLUListElement;

function searchbarHandler() {
    // Check if the current OS webview engine supports the modern CSS
    if (!CSS.supports('field-sizing', 'content')) {
        // Reset height to recalculate, then set it exactly to the scroll height
        input.style.height = 'auto';
        input.style.height = input.scrollHeight + 'px';
    }
    clear(suggestions);
    if (input.value.trim() === "") return; 
    let matches = fuse.search(input.value);
    if (matches.length != 0) display(suggestions);
    for (const match of matches) {
        const li = document.createElement('li');
        li.textContent = match.item;
        li.onclick = ()=> {
            input.value = li.textContent;
            clear(suggestions);
        }
        li?.classList.add("px-1", "py-0", "m-1", "cursor-pointer", "hover:text-white", "hover:bg-gray-800", "transition-colors");
        suggestions.appendChild(li);
    }
}
input.oninput = searchbarHandler;

const continuation_prompts = document.getElementById("continuation_prompts") as HTMLUListElement;
export function add_prompt_continuation(prompt_text: string) {
    const textarea = document.createElement('textarea');
    textarea.rows = 1;
    textarea.spellcheck=false;
    textarea.autocapitalize="off";
    textarea.autocomplete="off";
    textarea.setAttribute("autocorrect", "off");
    textarea.setAttribute("data-gramm", "false");
    textarea?.classList.add("p-2", "bg-gray-800", "rounded", "outline-none", "border-2", "inline-block",
        "border-transparent", "hover:border-slate-500", "focus:border-teal-500", "transition-colors");
    textarea.oninput = searchbarHandler;
    
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
    textarea.focus();
}