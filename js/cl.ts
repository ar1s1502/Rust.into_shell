import Fuse from 'fuse.js' //tokenize by word + fuzzy search using bitap algo
// import { invoke } from '@tauri-apps/api/core';

// invoke('init_shell').catch((err: any) => {
//     console.log(err);
// });

const items = [
    "as;fhas", "askjdfja", "llll", "uioiuio\nadf", "hjkjhjk", "bnmnbnbv", "trtytty", "Decadence\ndfd", "Tones", "Octet", "CJC", "dts", "air"
];
const fuse = new Fuse(items, {
    includeMatches: true,
    useTokenSearch: true,
});

const input = document.getElementById("cl") as HTMLTextAreaElement;
let suggestions = document.getElementById("suggestions") as HTMLUListElement;
// const cli_div = document.getElementById("cli_div") as HTMLDivElement;

function clear(ele: HTMLElement) {
    ele.innerHTML = "";
    ele.style.display = "none";
}

function display(ele: HTMLElement) {
    ele.style.display = "block";
}

function searchbar_handler() {
    clear(suggestions);
    if (input.value === "") return; 
    // if (input.value.endsWith("\n") {})
    let matches = fuse.search(input.value);
    if (matches.length != 0) display(suggestions);
    for (const match of matches) {
        const li = document.createElement('li');
        li.textContent = match.item;
        li.onclick = ()=> {
            input.value = li.textContent;
            clear(suggestions);
        }
        li?.classList.add("p-1", "m-1", "cursor-pointer", "hover:text-white", "hover:bg-gray-800", "transition-colors");
        suggestions.appendChild(li);
    }
}

input.oninput = searchbar_handler;