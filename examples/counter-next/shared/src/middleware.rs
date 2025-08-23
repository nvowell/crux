use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        mpsc::{Sender, channel},
    },
    thread::spawn,
};

use crux_core::{Request, RequestHandle, middleware::EffectMiddleware};
use crux_time::{TimeRequest, TimeResponse};
use rand::{
    Rng as _, SeedableRng, TryRngCore as _,
    rngs::{OsRng, StdRng},
};

use crate::capabilities::{RandomNumber, RandomNumberRequest};

#[allow(clippy::type_complexity)]
pub struct RngMiddleware {
    jobs_tx: Sender<(RandomNumberRequest, Box<dyn FnOnce(RandomNumber) + Send>)>,
}

impl RngMiddleware {
    pub fn new() -> Self {
        let (jobs_tx, jobs_rx) =
            channel::<(RandomNumberRequest, Box<dyn FnOnce(RandomNumber) + Send>)>();

        // Persistent background worker
        spawn(move || {
            let mut os_rng = OsRng;
            let mut rng = StdRng::seed_from_u64(os_rng.try_next_u64().expect("could not seed RNG"));

            while let Ok((RandomNumberRequest(from, to), callback)) = jobs_rx.recv() {
                #[allow(clippy::cast_sign_loss)]
                let top = (to - from) as usize;
                #[allow(clippy::cast_possible_wrap)]
                let out = rng.random_range(0..top) as isize + from;

                callback(RandomNumber(out));
            }
        });

        Self { jobs_tx }
    }
}

impl<Effect> EffectMiddleware<Effect> for RngMiddleware
where
    Effect: TryInto<Request<RandomNumberRequest>, Error = Effect>,
{
    type Op = RandomNumberRequest;

    fn try_process_effect_with(
        &self,
        effect: Effect,
        resolve_callback: impl FnOnce(RequestHandle<RandomNumber>, RandomNumber) + Send + 'static,
    ) -> Result<(), Effect> {
        let rand_request = effect.try_into()?;
        let (operation, handle): (RandomNumberRequest, _) = rand_request.split();

        self.jobs_tx
            .send((
                operation,
                Box::new(move |number| resolve_callback(handle, number)),
            ))
            .expect("Job failed to send to worker thread");

        Ok(())
    }
}

pub struct TimeMiddleware {
    jobs_tx: Sender<(
        TimeRequest,
        RequestHandle<TimeResponse>,
        Box<dyn Fn(RequestHandle<TimeResponse>, TimeResponse) + Send>,
    )>,
}

impl TimeMiddleware {
    pub fn new() -> Self {
        let (jobs_tx, jobs_rx) = channel::<(
            TimeRequest,
            RequestHandle<TimeResponse>,
            Box<dyn Fn(RequestHandle<TimeResponse>, TimeResponse) + Send>,
        )>();

        let timers: Arc<Mutex<HashMap<usize, Sender<()>>>> = Arc::new(Mutex::new(HashMap::new()));

        spawn(move || {
            while let Ok((req, handle, callback)) = jobs_rx.recv() {
                match req {
                    TimeRequest::Clear { id } => {
                        if let Some(timer) = timers.lock().unwrap().get(&id.0) {
                            let _ = timer.send(());
                        }
                        callback(handle, TimeResponse::Cleared { id });
                    }
                    TimeRequest::Interval { id, duration } => {
                        let (stop_tx, stop_rx) = channel::<()>();
                        timers.lock().unwrap().insert(id.0, stop_tx);

                        spawn(move || {
                            loop {
                                if stop_rx.recv_timeout(duration.into()).is_ok() {
                                    break;
                                }

                                let now = std::time::SystemTime::now();
                                callback(handle.clone(), TimeResponse::Tick {
                                    id,
                                    instant: crux_time::Instant::from(now),
                                });
                            }
                        });
                    }
                    _ => panic!("whoops"),
                }
            }
        });

        Self { jobs_tx }
    }
}

impl<Effect> EffectMiddleware<Effect> for TimeMiddleware
where
    Effect: TryInto<Request<TimeRequest>, Error = Effect>,
{
    type Op = TimeRequest;

    fn try_process_effect_with(
        &self,
        effect: Effect,
        resolve_callback: impl Fn(RequestHandle<TimeResponse>, TimeResponse) + Send + 'static,
    ) -> Result<(), Effect> {
        let time_request = effect.try_into()?;
        let (operation, handle): (TimeRequest, _) = time_request.split();

        self.jobs_tx
            .send((operation, handle, Box::new(resolve_callback)))
            .expect("Job failed to send to worker thread");

        Ok(())
    }
}
