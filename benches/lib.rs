#![feature(test)]

extern crate test;
use test::Bencher;

extern crate deque;
extern crate crossbeam;

use std::thread;
use std::sync::mpsc;
use std::sync::Arc;

const NUM_INTS: u32 = 1 << 16;
const NUM_VECS: u32 = 256;
const SIZE_VEC: usize = 4096;

#[bench]
fn bench_create_threads(b: &mut Bencher) {
    b.iter(|| {
        thread::spawn(|| {
        });

        thread::spawn(|| {
        }).join().unwrap();
    });
}

#[bench]
fn bench_alloc_vec(b: &mut Bencher) {
    b.iter(|| {
        let vec = vec![0; 4096];
        let mut sum = 0;
        for i in 0 .. 4096 {
            sum += vec[i];
        }
        assert!(vec.len() == 4096 + sum);
    });
}

#[bench]
fn bench_segqueue_ints(b: &mut Bencher) {
    b.iter(|| {
        use crossbeam::sync::SegQueue;

        let arc = Arc::new(SegQueue::new());
        let queue = arc.clone();
        thread::spawn(move || {
            for i in 0 .. NUM_INTS {
                queue.push(i);
            }
        });

        let queue = arc.clone();
        thread::spawn(move || {
            let mut i = 0;
            while i < NUM_INTS {
                if let Some(front) = queue.try_pop() {
                    assert!(front == i);
                    i += 1;
                }
            }
        }).join().unwrap();
    });
}

#[bench]
fn bench_segqueue_vecs(b: &mut Bencher) {
    b.iter(|| {
        use crossbeam::sync::SegQueue;

        let arc = Arc::new(SegQueue::new());
        let queue = arc.clone();
        thread::spawn(move || {
            for i in 0 .. NUM_VECS {
                queue.push(vec![0; SIZE_VEC]);
            }
        });

        let queue = arc.clone();
        thread::spawn(move || {
            let mut i = 0;
            while i < NUM_VECS {
                if let Some(front) = queue.try_pop() {
                    assert!(front.len() == SIZE_VEC);
                    i += 1;
                }
            }
        }).join().unwrap();
    });
}

#[bench]
fn bench_msqueue_ints(b: &mut Bencher) {
    b.iter(|| {
        use crossbeam::sync::MsQueue;

        let arc = Arc::new(MsQueue::new());
        let queue = arc.clone();
        thread::spawn(move || {
            for i in 0 .. NUM_INTS {
                queue.push(i);
            }
        });

        let queue = arc.clone();
        thread::spawn(move || {
            let mut i = 0;
            while i < NUM_INTS {
                if let Some(front) = queue.try_pop() {
                    assert!(front == i);
                    i += 1;
                }
            }
        }).join().unwrap();
    });
}

#[bench]
fn bench_msqueue_vecs(b: &mut Bencher) {
    b.iter(|| {
        use crossbeam::sync::MsQueue;

        let arc = Arc::new(MsQueue::new());
        let queue = arc.clone();
        thread::spawn(move || {
            for i in 0 .. NUM_VECS {
                queue.push(vec![0; SIZE_VEC]);
            }
        });

        let queue = arc.clone();
        thread::spawn(move || {
            let mut i = 0;
            while i < NUM_VECS {
                if let Some(front) = queue.try_pop() {
                    assert!(front.len() == SIZE_VEC);
                    i += 1;
                }
            }
        }).join().unwrap();
    });
}

#[bench]
fn bench_channel_ints(b: &mut Bencher) {
    b.iter(|| {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for i in 0 .. NUM_INTS {
                tx.send(i).unwrap();
            }
        });

        thread::spawn(move || {
            for i in 0 .. NUM_INTS {
                assert!(rx.recv().unwrap() == i);
            }
        }).join().unwrap();
    });
}

#[bench]
fn bench_channel_vec(b: &mut Bencher) {
    b.iter(|| {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for i in 0 .. NUM_VECS {
                tx.send(vec![0; SIZE_VEC]).unwrap();
            }
        });

        thread::spawn(move || {
            for i in 0 .. NUM_VECS {
                assert!(rx.recv().unwrap().len() == SIZE_VEC);
            }
        }).join().unwrap();
    });
}

#[bench]
fn bench_lock_ints(b: &mut Bencher) {
    use std::sync::{Arc, RwLock};
    use std::collections::VecDeque;

    b.iter(|| {
        let arc = Arc::new(RwLock::new(VecDeque::new()));
        let lock = arc.clone();
        thread::spawn(move || {
            for i in 0 .. NUM_INTS {
                if let Ok(mut guard) = lock.write() {
                    let deque = &mut guard;
                    deque.push_back(i);
                }
            }
        });

        let lock = arc.clone();
        thread::spawn(move || {
            let mut i = 0;
            while i < NUM_INTS {
                if let Ok(mut guard) = lock.write() {
                    let deque = &mut guard;
                    if let Some(front) = deque.pop_front() {
                        assert!(front == i);
                        i += 1;
                    }
                }
            }
        }).join().unwrap();
    });
}

#[bench]
fn bench_lock_vecs(b: &mut Bencher) {
    use std::sync::{Arc, RwLock};
    use std::collections::VecDeque;

    b.iter(|| {
        let arc = Arc::new(RwLock::new(VecDeque::new()));
        let lock = arc.clone();
        thread::spawn(move || {
            for i in 0 .. NUM_VECS {
                if let Ok(mut guard) = lock.write() {
                    let deque = &mut guard;
                    deque.push_back(vec![0; SIZE_VEC]);
                }
            }
        });

        let lock = arc.clone();
        thread::spawn(move || {
            let mut i = 0;
            while i < NUM_VECS {
                if let Ok(mut guard) = lock.write() {
                    let deque = &mut guard;
                    if let Some(front) = deque.pop_front() {
                        assert!(front.len() == SIZE_VEC);
                        i += 1;
                    }
                }
            }
        }).join().unwrap();
    });
}

#[bench]
fn bench_deque_ints(b: &mut Bencher) {
    b.iter(|| {
        let (worker, stealer) = deque::new();
        thread::spawn(move || {
            for i in 0 .. NUM_INTS {
                worker.push(i);
            }
        });

        thread::spawn(move || {
            let mut i = 0;
            while i < NUM_INTS {
                match stealer.steal() {
                    deque::Data(n) => {
                        assert!(n == i);
                        i += 1;
                    }
                    _ => ()
                }
            }
        }).join().unwrap();
    });
}

#[bench]
fn bench_deque_vecs(b: &mut Bencher) {
    b.iter(|| {
        let (worker, stealer) = deque::new();
        thread::spawn(move || {
            for i in 0 .. NUM_VECS {
                worker.push(vec![0; SIZE_VEC]);
            }
        });

        thread::spawn(move || {
            let mut i = 0;
            while i < NUM_VECS {
                match stealer.steal() {
                    deque::Data(n) => {
                        assert!(n.len() == SIZE_VEC);
                        i += 1;
                    }
                    _ => ()
                }
            }
        }).join().unwrap();
    });
}
