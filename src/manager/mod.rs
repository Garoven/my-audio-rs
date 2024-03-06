use std::sync::{atomic, Arc};
use tokio::sync::{
    mpsc::{channel, Sender},
    Mutex,
};

use crate::{Track, TrackSourceHandle, TracksQueue, TracksQueueHandle};

// Need to make OutputStream send
// I don't even use it. It need to be alive to keep audio alive
struct OutS(rodio::OutputStream);
unsafe impl Send for OutS {}

pub enum Request {
    Play(Play, bool),
    Pause,
    Resume,
    SetVolume(f32),
    Seek(u64),
    Queue(Queue),
}

impl std::fmt::Display for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Request::Pause => write!(f, "Pause"),
            Request::Play(ref inner, lazy) => write!(f, "Play(inner: {inner}, lazy: {lazy})"),
            Request::Resume => write!(f, "Resume"),
            Request::Seek(pos) => write!(f, "Seek(ts: {pos})"),
            Request::Queue(ref inner) => write!(f, "Queue(inner: {inner})"),
            Request::SetVolume(value) => write!(f, "SetVolume(value: {value})"),
        }
    }
}

#[derive(PartialEq)]
pub enum Play {
    Prev,
    Current,
    Next,
    Pos(usize),
}

impl std::fmt::Display for Play {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Play::Prev => write!(f, "Prev"),
            Play::Next => write!(f, "Next"),
            Play::Pos(pos) => write!(f, "Pos(pos: {pos})"),
            Play::Current => write!(f, "Current"),
        }
    }
}

#[derive(Debug)]
pub enum Queue {
    Clear,
    Add(String),
    Remove(usize),
    Swap(usize, usize),
}

impl std::fmt::Display for Queue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Queue::Clear => write!(f, "Queue"),
            Queue::Add(ref url) => write!(f, "Add(url: {url})",),
            Queue::Swap(from, to) => write!(f, "Swap(from: {from}, to: {to})"),
            Queue::Remove(pos) => write!(f, "Remove(pos: {pos})"),
        }
    }
}

pub struct AudioManager {
    rt: Arc<tokio::runtime::Runtime>,
    current_track: Arc<Mutex<Option<TrackSourceHandle>>>,
    queue: Arc<Mutex<TracksQueue>>,
    is_playing: Arc<atomic::AtomicBool>,
    tx: Sender<Request>,
}

impl AudioManager {
    pub fn new(rt: Arc<tokio::runtime::Runtime>) -> Self {
        AudioHandler::start(rt)
    }

    pub fn queue(&self) -> TracksQueueHandle {
        self.rt.block_on(self.queue.lock()).handle()
    }

    pub fn current_track(&self) -> Option<TrackSourceHandle> {
        self.rt
            .block_on(self.current_track.lock())
            .as_ref()
            .map(Clone::clone)
    }

    pub fn is_playing(&self) -> bool {
        self.is_playing.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn send(&self, request: Request) {
        self.rt.block_on(self.tx.send(request)).unwrap();
    }
}

#[derive(Clone)]
struct Context {
    pub current_track: Arc<Mutex<Option<TrackSourceHandle>>>,
    pub current_signal: Arc<Mutex<Option<std::sync::mpsc::Receiver<()>>>>,
    pub next_track: Arc<Mutex<Option<TrackSourceHandle>>>,
    pub next_signal: Arc<Mutex<Option<std::sync::mpsc::Receiver<()>>>>,
    pub sink: Arc<rodio::Sink>,
    pub is_playing: Arc<atomic::AtomicBool>,
    pub queue: Arc<Mutex<TracksQueue>>,
    pub input: Arc<Mutex<Option<Arc<rodio::queue::SourcesQueueInput<f32>>>>>,
}

struct AudioHandler;

impl AudioHandler {
    pub fn start(rt: Arc<tokio::runtime::Runtime>) -> AudioManager {
        let current_track = Arc::new(Mutex::new(None));
        let current_signal = Arc::new(Mutex::new(None));
        let next_track = Arc::new(Mutex::new(None));
        let next_signal = Arc::new(Mutex::new(None));
        let queue = Arc::new(Mutex::new(TracksQueue::new()));
        let is_playing = Arc::new(atomic::AtomicBool::new(false));
        let (output, output_handle) = rodio::OutputStream::try_default().unwrap();
        let sink = Arc::new(rodio::Sink::try_new(&output_handle).unwrap());
        let input = Arc::new(Mutex::new(None));

        let ctx = Context {
            current_track,
            current_signal,
            next_track,
            next_signal,
            sink,
            is_playing,
            queue,
            input,
        };

        let (tx, mut rx) = channel(20);
        let Context {
            current_track,
            queue,
            is_playing,
            ..
        } = ctx.clone();

        rt.spawn({
            let tx = tx.clone();
            let Context {
                current_track,
                current_signal,
                next_track,
                next_signal,
                sink,
                is_playing,
                queue,
                ..
            } = ctx.clone();

            async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

                    is_playing.store(!sink.is_paused(), std::sync::atomic::Ordering::Relaxed);
                    if let Some(ref track) = *current_track.lock().await {
                        let current_time = track.current_time();
                        let total_duration = track.metadata().duration;

                        if total_duration.saturating_sub(current_time) < 10
                            && next_track.lock().await.is_none()
                            && queue.lock().await.peek_next().is_some()
                        {
                            tx.send(Request::Play(Play::Next, true)).await.unwrap();
                        }
                    }

                    if let Some(ref mut current_signal) = *current_signal.lock().await {
                        match current_signal.try_recv() {
                            Ok(()) => {
                                {
                                    let mut queue_lock = queue.lock().await;
                                    let pos = queue_lock.pos();
                                    queue_lock.set_pos(pos + 1);
                                }

                                let bundle = (
                                    next_signal.lock().await.take(),
                                    next_track.lock().await.take(),
                                );

                                if let (Some(signal), Some(track)) = bundle {
                                    *current_signal = signal;
                                    *current_track.lock().await = Some(track);
                                } else {
                                    sink.pause();
                                }
                            }
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                let bundle = (
                                    next_signal.lock().await.take(),
                                    next_track.lock().await.take(),
                                );

                                if let (Some(signal), Some(track)) = bundle {
                                    *current_signal = signal;
                                    *current_track.lock().await = Some(track);
                                } else {
                                    sink.pause();
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        let output = OutS(output);
        rt.spawn(async move {
            // This can not be droped
            let _output = output;

            loop {
                let Some(request) = rx.recv().await else {
                    panic!("gui siadlo")
                };
                let ctx = ctx.clone();

                log::info!("Request - {request}");
                tokio::spawn(async move {
                    match request {
                        Request::Pause => Self::pause(ctx).await,
                        Request::Play(request, lazy) => Self::play(ctx, request, lazy).await,
                        Request::Resume => Self::resume(ctx).await,
                        Request::Seek(pos) => Self::seek(ctx, pos).await,
                        Request::Queue(request) => Self::queue(ctx, request).await,
                        Request::SetVolume(value) => Self::set_volume(ctx, value).await,
                    }
                });
            }
        });

        AudioManager {
            rt,
            current_track,
            queue,
            is_playing,
            tx,
        }
    }

    async fn pause(ctx: Context) {
        let Context { sink, .. } = ctx;

        sink.pause()
    }

    async fn play(ctx: Context, request: Play, lazy: bool) {
        let Context {
            current_track,
            current_signal,
            next_track,
            next_signal,
            sink,
            queue,
            input,
            ..
        } = ctx;

        let track = {
            let queue_lock = &mut queue.lock().await;
            match request {
                Play::Prev => queue_lock.prev_track(),
                Play::Current => queue_lock.current(),
                Play::Next => queue_lock.next_track(),
                Play::Pos(pos) => {
                    queue_lock.set_pos(pos);
                    queue_lock.current()
                }
            }
            .cloned()
        };

        if let Some(track) = track {
            if lazy && next_track.lock().await.is_none() && input.lock().await.is_some() {
                let Some((source, source_handle)) = track.start().await else {
                    return;
                };

                {
                    let queue = &mut queue.lock().await;
                    let pos = queue.pos() - 1;
                    queue.set_pos(pos)
                }

                {
                    let input_lock = input.lock().await;
                    let input = input_lock.as_ref().unwrap();
                    *next_signal.lock().await = Some(input.append_with_signal(source));
                }

                *next_track.lock().await = Some(source_handle);
            } else {
                let Some((source, source_handle)) = track.start().await else {
                    return;
                };

                let (new_input, output) = rodio::queue::queue::<f32>(true);

                sink.stop();
                sink.append(output);
                sink.play();

                *current_signal.lock().await = Some(new_input.append_with_signal(source));
                *current_track.lock().await = Some(source_handle);
                *next_signal.lock().await = None;
                *next_track.lock().await = None;
                *input.lock().await = Some(new_input);
            }
        }
    }

    async fn resume(ctx: Context) {
        let Context { sink, .. } = ctx;

        sink.play()
    }

    async fn seek(ctx: Context, pos: u64) {
        let Context {
            sink,
            current_track,
            ..
        } = ctx;

        if let Some(track) = &*current_track.lock().await {
            sink.pause();
            track.seek(pos, 0.0);
            sink.play();
        };
    }

    async fn queue(ctx: Context, request: Queue) {
        let Context { queue, .. } = ctx;

        match request {
            Queue::Clear => queue.lock().await.clear(),
            Queue::Add(url) => {
                if let Some(track) = Track::new(url).await {
                    queue.lock().await.push_back(track);
                }
            }
            Queue::Remove(pos) => _ = queue.lock().await.remove(pos),
            Queue::Swap(from, to) => queue.lock().await.swap(from, to),
        }
    }

    async fn set_volume(ctx: Context, value: f32) {
        let Context { sink, .. } = ctx;

        sink.set_volume(value)
    }
}
