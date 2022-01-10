use crate::generator::Idgend;
use actix_http::Response;
use actix_web::{HttpRequest, HttpResponse, Responder};
use anyhow::bail;
use chrono::Local;
use num_traits::cast::ToPrimitive;
use rand::random;
use std::future::{ready, Ready};

// temp var for test, 2018-01-01 00:00:00
pub const STANDARD_EPOCH: u64 = 1514736000_000u64;

// shift
const WORKER_ID_SHIFT: u8 = 12;
const TIMESTAMP_LEFT_SHIFT: u8 = 22;

// mask
const SEQUENCE_MASK: u16 = 0xFFF;

#[derive(Debug, Default, Copy, Clone)]
pub struct Snowflake {
    worker_id: u16,
    sequence: u16,
    last_timestamp: u64,
}

#[derive(Debug)]
pub struct SnowFlakeId(u64);

#[allow(dead_code)]
impl SnowFlakeId {
    fn snowflake_timestamp(&self) -> u64 {
        (self.0 >> TIMESTAMP_LEFT_SHIFT) + STANDARD_EPOCH
    }
    fn worker_id(&self) -> u16 {
        (self.0 >> (WORKER_ID_SHIFT) & 0b11_1111_1111) as u16
    }
    fn sequence(&self) -> u16 {
        (self.0 & 0b1111_1111_1111) as u16
    }
}

impl Responder for SnowFlakeId {
    type Error = actix_web::Error;
    type Future = Ready<Result<Response, actix_web::Error>>;
    fn respond_to(self, _: &HttpRequest) -> Self::Future {
        ready(Ok(HttpResponse::Ok()
            .content_type("text/plain; charset=utf-8")
            .body(format!("{}", self.0))))
    }
}

impl Snowflake {
    pub fn new(worker_id: u16) -> Self {
        Snowflake {
            worker_id: worker_id & 0b11_1111_1111_u16,
            sequence: 0,
            last_timestamp: 0,
        }
    }

    fn wait_for_next_milli_sec(&self) -> u64 {
        let mut curr_timestamp = Snowflake::current_timestamp_millis();
        while self.last_timestamp >= curr_timestamp {
            curr_timestamp = Snowflake::current_timestamp_millis();
        }
        curr_timestamp
    }

    fn current_timestamp_millis() -> u64 {
        Local::now().timestamp_millis().to_u64().unwrap()
    }
}

impl Idgend<SnowFlakeId> for Snowflake {
    fn get(&mut self, jump: bool) -> anyhow::Result<SnowFlakeId> {
        let mut current_timestamp = Snowflake::current_timestamp_millis();
        if current_timestamp < self.last_timestamp {
            bail!(format!(
                "Clock moved backwards. Refusing to generate id for {} milliseconds",
                self.last_timestamp
            ));
        }

        if jump {
            self.sequence = (self.sequence + 1 + random::<u16>() % 5) & SEQUENCE_MASK;
        } else {
            self.sequence = (self.sequence + 1) & SEQUENCE_MASK;
        }

        if current_timestamp == self.last_timestamp {
            if self.sequence == 0 {
                current_timestamp = self.wait_for_next_milli_sec();
            }
        } else {
            self.sequence = 0_u16;
        }

        self.last_timestamp = current_timestamp;

        Ok(SnowFlakeId(
            (current_timestamp - STANDARD_EPOCH) << TIMESTAMP_LEFT_SHIFT
                | (self.worker_id as u64) << WORKER_ID_SHIFT
                | (self.sequence as u64),
        ))
    }
}

#[cfg(test)]
mod test {
    use crate::generator::{Idgend, Snowflake};
    use std::time::Instant;

    #[test]
    fn test_fn() {
        let mut id_gen = Snowflake::new(0);
        let id = id_gen.get(false);
        assert!(id.is_ok());
        let id = id.unwrap();
        let seq = id.sequence();
        let work_id = id.worker_id();
        let time1 = id.snowflake_timestamp();
        println!("time: {}, work_id: {}, seq:{} ", time1, work_id, seq);
        assert_eq!(0, work_id);
        assert_eq!(0, seq);

        let id = id_gen.get(false);
        assert!(id.is_ok());
        let id = id.unwrap();
        let seq = id.sequence();
        let work_id = id.worker_id();
        let time2 = id.snowflake_timestamp();
        println!("time: {}, work_id: {}, seq:{} ", time2, work_id, seq);
        assert_eq!(0, work_id);

        assert_eq!(if time1 == time2 { 1 } else { 0 }, seq);
    }

    #[test]
    fn loop_test() {
        let mut id_gen = Snowflake::new(0);
        let now = Instant::now();
        for _ in 0..=1000000 {
            let out = id_gen.get(true);
            assert!(out.is_ok());
        }
        let elapsed = now.elapsed();
        println!(
            "single thread generate 1000 ids cost {}.{:09} s",
            elapsed.as_secs(),
            elapsed.subsec_nanos()
        );
    }
}
