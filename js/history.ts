import { invoke, } from '@tauri-apps/api/core';
import Fuse from 'fuse.js' //tokenize by word + fuzzy search using bitap algo

let history: string[];
let ptr: number = 0;
let fuse: Fuse<string>;
const MAX_LENGTH = 25;
invoke('get_editor_hist').then((hist_) => {
    history = hist_ as string[] ?? [];
    history.slice(-MAX_LENGTH);
    fuse = new Fuse(history, {
        includeMatches: true,
        useTokenSearch: true,
    });
    console.log(history, history.length);
});

export function add_to_hist(cmd: string) {
    const idx: number = ptr % MAX_LENGTH;
    fuse.removeAt(idx);
    history[idx] = cmd;
    ptr++;
    fuse.add(cmd);
}

export function fuzzy_search(query: string) {
    if (fuse !== null) {
        return fuse.search(query);
    }
}