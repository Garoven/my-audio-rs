use crate::Track;

#[derive(Default)]
pub struct TracksQueue {
    inner: std::collections::VecDeque<Track>,
    pos: usize,
}

impl TracksQueue {
    pub fn new() -> Self {
        Self {
            inner: Default::default(),
            pos: 0,
        }
    }

    pub fn handle(&self) -> TracksQueueHandle {
        TracksQueueHandle {
            inner: self.inner.clone(),
            pos: self.pos,
        }
    }

    pub fn clear(&mut self) {
        self.inner.clear();
        self.pos = 0;
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos.clamp(0, self.len())
    }

    pub fn push_back(&mut self, track: Track) {
        self.inner.push_back(track);
    }

    pub fn push_front(&mut self, track: Track) {
        self.inner.push_front(track);
    }

    pub fn remove(&mut self, index: usize) -> Option<Track> {
        if index == self.pos {
            self.pos -= 1;
        }
        self.inner.remove(index)
    }

    pub fn current(&self) -> Option<&Track> {
        self.inner.get(self.pos)
    }

    pub fn next_track(&mut self) -> Option<&Track> {
        if self.pos >= self.len().saturating_sub(1) {
            return None;
        }

        self.pos += 1;
        self.inner.get(self.pos)
    }

    pub fn peek_next(&mut self) -> Option<&Track> {
        if self.pos >= self.len().saturating_sub(1) {
            return None;
        }

        self.inner.get(self.pos + 1)
    }

    pub fn prev_track(&mut self) -> Option<&Track> {
        if self.pos == 0 {
            return None;
        }

        self.pos -= 1;
        self.inner.get(self.pos)
    }

    pub fn peek_prev(&mut self) -> Option<&Track> {
        if self.pos == 0 {
            return None;
        }

        self.inner.get(self.pos - 1)
    }

    pub fn swap(&mut self, from: usize, to: usize) {
        self.inner.swap(from, to)
    }
}

#[derive(Default)]
pub struct TracksQueueHandle {
    inner: std::collections::VecDeque<Track>,
    pos: usize,
}

impl TracksQueueHandle {
    pub fn list(&self) -> &std::collections::VecDeque<Track> {
        &self.inner
    }

    pub fn pos(&self) -> usize {
        self.pos
    }
}
