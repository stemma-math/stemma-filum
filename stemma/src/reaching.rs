//! Opening a connection to an instance without waiting on a dead address.
//!
//! A host name carries several addresses, and on a machine with a route
//! into a family it cannot actually cross — a tunnel that is up but
//! carries nothing, most often — one of them accepts a connection that
//! never completes. The client underneath tries them in turn and gives
//! the first the largest share of the budget, so every request pays
//! that whole share before the address that works is even tried.
//!
//! So the addresses are ordered here before they are handed over: the
//! one that answers first is the one that goes first. Browsers and curl
//! call this happy eyeballs.

use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::NextTimeout;
use ureq::{Error, config::Config};

/// How long the addresses are raced before giving up on telling them
/// apart.
///
/// This is spent only when a connection is actually being opened, and
/// what it buys is not having to spend the connection budget instead.
/// An instance that cannot answer within it is one the request was
/// going to struggle with anyway.
const RACE: Duration = Duration::from_secs(2);

/// How many addresses are raced.
///
/// A name that carries more than a few is a load balancer's, and the
/// ones past the first of each family say nothing new about whether
/// the family works.
const RACED: usize = 4;

/// Resolves a name and puts the address that answers first at the front.
#[derive(Debug, Default)]
pub struct HappyEyeballs {
    inner: DefaultResolver,
}

impl Resolver for HappyEyeballs {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, Error> {
        let addresses = self.inner.resolve(uri, config, timeout)?;

        Ok(match first_to_answer(&addresses) {
            Some(first) => ahead_of_the_rest(addresses, first),
            None => addresses,
        })
    }
}

/// Which address answers first, where they do not all belong to one
/// family.
///
/// One family means nothing to choose between, and the addresses are
/// left as the system ordered them: it knows things about the machine's
/// routes that a connection attempt would only rediscover.
fn first_to_answer(addresses: &ResolvedSocketAddrs) -> Option<SocketAddr> {
    let mixed = addresses
        .iter()
        .any(|address| address.is_ipv4() != addresses[0].is_ipv4());

    if !mixed {
        return None;
    }

    race(addresses.iter().copied().take(RACED))
}

/// Opens all of them at once and keeps whichever connects.
///
/// The losing attempts are dropped, which closes them. What is learned
/// is the address, not the connection: handing a socket over is not
/// something the client underneath offers, and opening one again costs
/// far less than waiting on one that never opens.
fn race(addresses: impl Iterator<Item = SocketAddr>) -> Option<SocketAddr> {
    let (say, heard) = mpsc::channel();

    for address in addresses {
        let say = say.clone();

        // Nothing joins these. A thread whose address never answers ends
        // when the attempt times out, and by then nobody is listening.
        std::thread::spawn(move || {
            if TcpStream::connect_timeout(&address, RACE).is_ok() {
                let _ = say.send(address);
            }
        });
    }

    drop(say);

    // Every attempt failing closes the channel, which ends this as
    // surely as the deadline does.
    heard.recv_timeout(RACE).ok()
}

/// The same addresses, with that one first.
fn ahead_of_the_rest(addresses: ResolvedSocketAddrs, first: SocketAddr) -> ResolvedSocketAddrs {
    let mut ordered = ResolvedSocketAddrs::from_fn(|_| first);

    ordered.push(first);

    for address in addresses
        .iter()
        .copied()
        .filter(|address| *address != first)
    {
        ordered.push(address);
    }

    ordered
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    fn addresses(given: &[&str]) -> ResolvedSocketAddrs {
        let given: Vec<SocketAddr> = given.iter().map(|one| one.parse().unwrap()).collect();
        let mut addresses = ResolvedSocketAddrs::from_fn(|_| given[0]);

        for one in given {
            addresses.push(one);
        }

        addresses
    }

    #[test]
    fn the_address_that_answers_is_put_first_and_none_are_lost() {
        let listening = TcpListener::bind("127.0.0.1:0").unwrap();
        let answers = listening.local_addr().unwrap();
        // Nothing is listening here: the port belongs to a listener that
        // has just been dropped.
        let silent = {
            let taken = TcpListener::bind("127.0.0.1:0").unwrap();
            taken.local_addr().unwrap()
        };

        let ordered = ahead_of_the_rest(
            addresses(&[&silent.to_string(), &answers.to_string()]),
            answers,
        );

        assert_eq!(ordered[0], answers);
        assert_eq!(ordered.len(), 2);
        assert!(ordered.contains(&silent));
    }

    #[test]
    fn one_family_is_left_in_the_order_the_system_gave() {
        let only_one_family = addresses(&["127.0.0.1:1", "127.0.0.2:2"]);

        assert_eq!(first_to_answer(&only_one_family), None);
    }

    #[test]
    fn the_one_that_answers_wins_the_race() {
        let listening = TcpListener::bind("127.0.0.1:0").unwrap();
        let answers = listening.local_addr().unwrap();

        assert_eq!(race([answers].into_iter()), Some(answers));
    }
}
