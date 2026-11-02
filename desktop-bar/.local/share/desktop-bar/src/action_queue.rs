//! Collapse consecutive replaceable values while preserving action barriers.
use std::sync::mpsc;

pub fn run<T>(
    receiver: mpsc::Receiver<T>,
    mut replaces: impl FnMut(&T, &T) -> bool,
    mut execute: impl FnMut(T) -> bool,
) {
    let mut next = None;
    while let Some(mut action) = next.take().or_else(|| receiver.recv().ok()) {
        while let Ok(newer) = receiver.try_recv() {
            if replaces(&action, &newer) {
                action = newer;
            } else {
                next = Some(newer);
                break;
            }
        }
        if !execute(action) {
            break;
        }
    }
}
