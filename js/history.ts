import { invoke, } from '@tauri-apps/api/core';
import { add_hist_li } from './cl.ts';
import Fuse from 'fuse.js' //tokenize by word + fuzzy search using bitap algo

let history: string[];
const MAX_LENGTH = 25;
let ptr = 0;

let fuse: Fuse<string> | null = null;

invoke('get_editor_hist').then((hist_) => {
    history = (hist_ as string[] ?? []).slice(-MAX_LENGTH);
    fuse = new Fuse(history, {
        includeMatches: true,
        useTokenSearch: true,
    });
    ptr = history.length;
    console.log(history, history.length);
    for (const entry of history) {
        add_hist_li(entry);
    }
    requestAnimationFrame(() => { //wait for DOM to render before scrolling to bottom of history list
        const hist_panel = document.getElementById('top_panel') as HTMLDivElement;
        hist_panel.scrollTop = hist_panel.scrollHeight;
    });
});

export function add_to_hist(cmd: string) {
    if (!fuse) return;
    if (history.length >= MAX_LENGTH) {
        history.shift();
        fuse.removeAt(0);
    }
    history.push(cmd);
    fuse.add(cmd);
    ptr = history.length;
}

export function fuzzy_search(query: string) {
    if (fuse) {
        return fuse.search(query);
    } 
    return null;
}

export function get_hist_entry(up: boolean) {
    if (history.length === 0) return null;
    if (up) {
        ptr = Math.max(0, ptr - 1);
    } else {
        ptr = Math.min(history.length, ptr + 1);
    }
    if (ptr === history.length) return null;
    return history[ptr];
}