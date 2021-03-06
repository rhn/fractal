extern crate serde_json;

use globals;
use std::thread;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;
use error::Error;
use util::json_q;
use util::get_user_avatar;
use util::get_user_avatar_img;
use backend::types::BKResponse;
use backend::types::Backend;

use types::Member;

use self::serde_json::Value as JsonValue;


macro_rules! semaphore {
    ($cv: expr, $blk: block) => {{
        let thread_count = $cv.clone();
        thread::spawn(move || {
            // waiting, less than 20 threads at the same time
            // this is a semaphore
            // TODO: use std::sync::Semaphore when it's on stable version
            // https://doc.rust-lang.org/1.1.0/std/sync/struct.Semaphore.html
            let &(ref num, ref cvar) = &*thread_count;
            {
                let mut start = num.lock().unwrap();
                while *start >= 20 {
                    start = cvar.wait(start).unwrap()
                }
                *start += 1;
            }

            $blk

            // freeing the cvar for new threads
            {
                let mut counter = num.lock().unwrap();
                *counter -= 1;
            }
            cvar.notify_one();
        });
    }}
}


pub fn get_username(bk: &Backend) -> Result<(), Error> {
    let id = bk.data.lock().unwrap().user_id.clone();
    let url = bk.url(&format!("profile/{}/displayname", id.clone()), vec![])?;
    let tx = bk.tx.clone();
    get!(&url,
        |r: JsonValue| {
            let name = String::from(r["displayname"].as_str().unwrap_or(&id));
            tx.send(BKResponse::Name(name)).unwrap();
        },
        |err| { tx.send(BKResponse::UserNameError(err)).unwrap() }
    );

    Ok(())
}

pub fn get_avatar(bk: &Backend) -> Result<(), Error> {
    let baseu = bk.get_base_url()?;
    let userid = bk.data.lock().unwrap().user_id.clone();

    let tx = bk.tx.clone();
    thread::spawn(move || match get_user_avatar(&baseu, &userid) {
        Ok((_, fname)) => {
            tx.send(BKResponse::Avatar(fname)).unwrap();
        }
        Err(err) => {
            tx.send(BKResponse::AvatarError(err)).unwrap();
        }
    });

    Ok(())
}

pub fn get_user_info_async(bk: &mut Backend,
                           uid: &str,
                           tx: Sender<(String, String)>)
                           -> Result<(), Error> {
    let baseu = bk.get_base_url()?;

    let u = String::from(uid);

    if let Some(info) = bk.user_info_cache.get(&u) {
        let i = info.lock().unwrap().clone();
        if !i.0.is_empty() || !i.1.is_empty() {
            tx.send(i).unwrap();
            return Ok(())
        }
    }

    let info = Arc::new(Mutex::new((String::new(), String::new())));
    let cache_key = u.clone();
    let cache_value = info.clone();

    semaphore!(bk.limit_threads, {
        let i0 = info.lock();
        match get_user_avatar(&baseu, &u) {
            Ok(info) => {
                tx.send(info.clone()).unwrap();
                let mut i = i0.unwrap();
                i.0 = info.0;
                i.1 = info.1;
            }
            Err(_) => {
                tx.send((String::new(), String::new())).unwrap();
            }
        };
    });

    bk.user_info_cache.insert(cache_key, cache_value);

    Ok(())
}

pub fn get_avatar_async(bk: &Backend, member: Option<Member>, tx: Sender<String>) -> Result<(), Error> {
    let baseu = bk.get_base_url()?;

    if member.is_none() {
        tx.send(String::new()).unwrap();
        return Ok(());
    }

    let m = member.unwrap();

    let uid = m.uid.clone();
    let alias = m.get_alias().clone();
    let avatar = m.avatar.clone();

    semaphore!(bk.limit_threads, {
        match get_user_avatar_img(&baseu, uid,
                                  alias.unwrap_or_default(),
                                  avatar.unwrap_or_default()) {
            Ok(fname) => { tx.send(fname.clone()).unwrap(); }
            Err(_) => { tx.send(String::new()).unwrap(); }
        }
    });

    Ok(())
}
