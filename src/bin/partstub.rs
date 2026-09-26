//! Tiny valid executable used as the base for `*-partN.exe` payload carriers.
//! These files are never meant to be run: the packed builder reads their
//! appended payload indexes directly. Double-clicking one only shows this note.

fn main() {
    println!("re3hd Builder - data part");
    println!("This file is part of the re3hd Builder release (like FitGirl's part files).");
    println!("Do not run or delete it - it must stay next to re3hd-builder-packed.exe.");
}