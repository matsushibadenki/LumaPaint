//! FIFO between canvases, newest pending request within a canvas. A busy editor
//! must never replace another window's work or retain many full document copies.
use std::{
    collections::VecDeque,
    sync::{
        mpsc::{RecvError, TrySendError},
        Arc, Condvar, Mutex,
    },
};
struct State<J> {
    pending: VecDeque<(u64, J)>,
    stopped: bool,
}
struct Shared<J> {
    state: Mutex<State<J>>,
    wake: Condvar,
}
pub(crate) struct LatestSender<J>(Arc<Shared<J>>);
pub(crate) struct LatestReceiver<J>(Arc<Shared<J>>);
pub(crate) fn channel<J>() -> (LatestSender<J>, LatestReceiver<J>) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            pending: VecDeque::new(),
            stopped: false,
        }),
        wake: Condvar::new(),
    });
    (LatestSender(shared.clone()), LatestReceiver(shared))
}
impl<J> LatestSender<J> {
    pub(crate) fn try_send(&self, canvas: u64, job: J) -> Result<(), TrySendError<J>> {
        let mut state = self.0.state.lock().unwrap();
        if state.stopped {
            return Err(TrySendError::Disconnected(job));
        }
        if let Some((_, pending)) = state.pending.iter_mut().find(|(key, _)| *key == canvas) {
            *pending = job;
        } else {
            state.pending.push_back((canvas, job));
        }
        self.0.wake.notify_one();
        Ok(())
    }
}
impl<J> LatestReceiver<J> {
    pub(crate) fn recv(&self) -> Result<J, RecvError> {
        let mut state = self.0.state.lock().unwrap();
        loop {
            if let Some((_, job)) = state.pending.pop_front() {
                return Ok(job);
            }
            if state.stopped {
                return Err(RecvError);
            }
            state = self.0.wake.wait(state).unwrap();
        }
    }
}
impl<J> Drop for LatestSender<J> {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().stopped = true;
        self.0.wake.notify_all();
    }
}
impl<J> Drop for LatestReceiver<J> {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.stopped = true;
        state.pending.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coalesces_one_canvas_without_starving_other_windows() {
        let (sender, receiver) = channel();
        sender.try_send(1, "old A").unwrap();
        sender.try_send(2, "B").unwrap();
        sender.try_send(1, "latest A").unwrap();
        sender.try_send(3, "C").unwrap();
        assert_eq!(receiver.recv().unwrap(), "latest A");
        sender.try_send(1, "next A").unwrap();
        assert_eq!(receiver.recv().unwrap(), "B");
        assert_eq!(receiver.recv().unwrap(), "C");
        assert_eq!(receiver.recv().unwrap(), "next A");
        drop(sender);
        assert!(receiver.recv().is_err());
    }
    #[test]
    fn stopped_worker_rejects_jobs_and_releases_payloads() {
        let (sender, receiver) = channel::<Vec<u8>>();
        sender.try_send(1, vec![1; 128]).unwrap();
        drop(receiver);
        assert!(matches!(
            sender.try_send(2, vec![2]),
            Err(TrySendError::Disconnected(_))
        ));
    }
}
