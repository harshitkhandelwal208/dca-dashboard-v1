//! Typed access to slash-command options.

use serenity::all::*;

pub type Opts<'a> = [ResolvedOption<'a>];

pub fn subcommand<'a>(opts: &'a Opts<'a>) -> Option<(&'a str, &'a Opts<'a>)> {
    opts.iter().find_map(|o| match &o.value {
        ResolvedValue::SubCommand(inner) => Some((o.name, inner.as_slice())),
        _ => None,
    })
}

pub fn get<'a>(opts: &'a Opts<'a>, name: &str) -> Option<&'a ResolvedValue<'a>> {
    opts.iter().find(|o| o.name == name).map(|o| &o.value)
}

pub fn string<'a>(opts: &'a Opts<'a>, name: &str) -> Option<&'a str> {
    match get(opts, name)? {
        ResolvedValue::String(s) => Some(*s),
        _ => None,
    }
}

pub fn integer(opts: &Opts, name: &str) -> Option<i64> {
    match get(opts, name)? {
        ResolvedValue::Integer(i) => Some(*i),
        _ => None,
    }
}

pub fn boolean(opts: &Opts, name: &str) -> Option<bool> {
    match get(opts, name)? {
        ResolvedValue::Boolean(b) => Some(*b),
        _ => None,
    }
}

pub fn user<'a>(opts: &'a Opts<'a>, name: &str) -> Option<&'a User> {
    match get(opts, name)? {
        ResolvedValue::User(u, _) => Some(*u),
        _ => None,
    }
}

pub fn role<'a>(opts: &'a Opts<'a>, name: &str) -> Option<&'a Role> {
    match get(opts, name)? {
        ResolvedValue::Role(r) => Some(*r),
        _ => None,
    }
}

pub fn channel<'a>(opts: &'a Opts<'a>, name: &str) -> Option<&'a PartialChannel> {
    match get(opts, name)? {
        ResolvedValue::Channel(c) => Some(*c),
        _ => None,
    }
}

pub fn attachment<'a>(opts: &'a Opts<'a>, name: &str) -> Option<&'a Attachment> {
    match get(opts, name)? {
        ResolvedValue::Attachment(a) => Some(*a),
        _ => None,
    }
}
