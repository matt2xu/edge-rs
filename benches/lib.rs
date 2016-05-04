#![feature(test)]

extern crate test;
use test::Bencher;

use std::thread;
use std::sync::mpsc;

#[bench]
fn bench_avec_alloc(b: &mut Bencher) {
    b.iter(|| {
        let mut vec = vec![0; 4096];
        let mut sum = 0;
        for i in 0 .. 4096 {
            sum += vec[i];
        }
        assert!(vec.len() == 4096 + sum);
    });
}

#[bench]
fn bench_channel(b: &mut Bencher) {
    b.iter(|| {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for i in 0 .. 10 {
                tx.send(i).unwrap();
            }
        });

        thread::spawn(move || {
            for i in 0 .. 10 {
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
            for i in 0 .. 10 {
                tx.send(vec![0; 4096]).unwrap();
            }
        });

        thread::spawn(move || {
            for i in 0 .. 10 {
                assert!(rx.recv().unwrap().len() == 4096);
            }
        }).join().unwrap();
    });
}

#[bench]
fn bench_lock(b: &mut Bencher) {
    use std::sync::{Arc, RwLock};
    use std::collections::VecDeque;

    b.iter(|| {
        let arc = Arc::new(RwLock::new(VecDeque::new()));
        let lock = arc.clone();
        thread::spawn(move || {
            for i in 0 .. 10 {
                if let Ok(mut guard) = lock.write() {
                    let deque = &mut guard;
                    deque.push_back(i);
                }
            }
        });

        let lock = arc.clone();
        thread::spawn(move || {
            let mut i = 0;
            while i < 10 {
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
fn bench_lock_vec(b: &mut Bencher) {
    use std::sync::{Arc, RwLock};
    use std::collections::VecDeque;

    b.iter(|| {
        let arc = Arc::new(RwLock::new(VecDeque::new()));
        let lock = arc.clone();
        thread::spawn(move || {
            for i in 0 .. 10 {
                if let Ok(mut guard) = lock.write() {
                    let deque = &mut guard;
                    deque.push_back(vec![0; 4096]);
                }
            }
        });

        let lock = arc.clone();
        thread::spawn(move || {
            let mut i = 0;
            while i < 10 {
                if let Ok(mut guard) = lock.write() {
                    let deque = &mut guard;
                    if let Some(front) = deque.pop_front() {
                        assert!(front.len() == 4096);
                        i += 1;
                    }
                }
            }
        }).join().unwrap();
    });
}

#[bench]
fn bench_deque(b: &mut Bencher) {
    extern crate deque;

    b.iter(|| {
        let (worker, stealer) = deque::new();
        thread::spawn(move || {
            for i in 0 .. 10 {
                worker.push(i);
            }
        });

        thread::spawn(move || {
            let mut i = 0;
            while i < 10 {
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
