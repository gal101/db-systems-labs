//! Implementation of the Exchange operator for the physical query plan.
//! The Exchange operator has the same interface of the classic single-node processing
//! operators (open, close, next). However, the actions performed in the `next` method
//! involve network communications (in this lab we use rust 'channels' as an abstraction
//! instead of the real network).
//! As suggested by the name, the responsibility of the Exchange operator is to exchange
//! data between different partitions to achieve distributed query processing.
//! How data are distributed is defined by the `DistributionFn` field.
//! As suggested by the name, the responsibility of the Exchange operator is to exchange
//! data between different partitions to achieve distributed query processing.
//!
//! # Arguments:
//! * `child`: operator that comes before the Exchange in the query plan.
//! * `peer_id`: id that identifies the current node in the network.
//! * `com_init`: manager of the communication. Used to retrieve the correct communication
//!    channel for the current peer.
//! * `distribution`: distribution strategy, defines how data are shuffled between nodes.
//!
//! The tricky part of this exercise is correctly managing the `next` method of the
//! operator.
//! There are multiple ways of generating a result:
//! - using local data (e.g., a tablescan).
//! - receiving data from other partitions.
//!
//! Both should be considered in your implementation.
//! ** ALWAYS PRIORITIZE LOCAL DATA TO PRODUCE A RESULT **. If no local data is available,
//! check if messages are available.

use super::Exchange;
use crate::dist_operator::distribution::DistributionFn;
use crate::network::CommunicationInitializer;
use crate::{DynOperator, Operator, Record};

impl Exchange {
    pub fn new(
        child: DynOperator,
        peer_id: u16,
        com_init: Box<dyn CommunicationInitializer>,
        distribution: DistributionFn,
    ) -> Box<Self> {
        Box::new(
            Self {
                child,
                peer_id,
                com_init,
                com: None,
                distribution
            }
        )
    }

    /// While not mandatory, we suggest to use the following two helper functions:
    /// * `send`
    /// * `receive`
    /// You can, of course, ignore them and define what you believe is correct/easier.
    ///
    /// `send` is responsible for sending data to other nodes based
    /// on the strategy defined by the `distribution` field.
    /// If the distribution strategy indicates that the current node should be the
    /// receiver, `send` returns the record. Otherwise it returns `None`.
    ///
    /// Hint: do not forget what you have learned so far. Pipelined execution is still
    /// valid.
    fn send(&mut self) -> Option<Record> {
        let com = self.com.as_mut().expect("Not opened!");
        loop {
            //get next record from child operator or close the communication and return None
            let Some(record) = self.child.next() else {
                com.close_send();
                return None
            };

            let peers_to_send = (self.distribution)(&record, com.peers());

            for peer in peers_to_send.iter() {
                if *peer == self.peer_id {
                    continue;
                }
                com.send(record.clone(), *peer);
            }

            if peers_to_send.contains(&self.peer_id) {
                return Some(record)
            }
        }
    }

    /// `receive` listens for messages from other nodes in the network.
    /// If a message is available, it returns it.
    fn receive(&mut self) -> Option<Record> {
        let com = self.com.as_mut().expect("Not opened!");
        if com.are_all_closed() {
            return None
        }
        com.receive()
    }
}

impl Iterator for Exchange {
    type Item = Record;

    fn next(&mut self) -> Option<Self::Item> {
        let record = self.send();
        match record {
            Some(_) => record,
            None => self.receive()
        }
    }
}

impl Operator for Exchange {
    fn open(&mut self) {
        self.child.open();

        let com = self.com_init.take_com(self.peer_id);
        self.com = Some(com);
    }

    fn close(&mut self) {
        self.child.close();

        self.com = None;
    }
}
