use std::{borrow::Cow, collections::HashMap};

use nom::{
    branch::alt,
    bytes::complete::{escaped_transform, is_not, tag, take, take_until, take_while1},
    character::complete::{alphanumeric1, char, digit0, digit1, multispace1},
    combinator::{cut, fail, map, map_opt, peek, value},
    error::context,
    multi::{fold_many0, many_till},
    sequence::{delimited, preceded},
    AsChar,
};

use crate::{command::PartialCommand, subsystem::shell::IResult};

#[derive(Debug, PartialEq, Eq)]
pub enum IterState<'a> {
    Expand(PartialCommand<'a>),
    Ready(PartialCommand<'a>),
}

#[derive(Debug)]
pub struct Iter<'a> {
    command: std::vec::IntoIter<ParsedPart<'a>>,
    expanding: Option<Box<Iter<'a>>>,
    stdio_out: [RedirectionTo<'a>; 2],
    exec: Option<Cow<'a, [u8]>>,
    params: Vec<Cow<'a, [u8]>>,
}

impl<'a> Iter<'a> {
    pub fn new(command: Vec<ParsedPart<'a>>) -> Self {
        Self {
            command: command.into_iter(),
            expanding: None,
            stdio_out: [
                RedirectionTo::Stdio(0), // stdout
                RedirectionTo::Stdio(1), // stderr
            ],
            exec: None,
            params: Vec::new(),
        }
    }
}

impl<'a> Iter<'a> {
    pub fn step(
        &mut self,
        env: &HashMap<Cow<'static, [u8]>, Cow<'static, [u8]>>,
        mut previous_out: Option<Vec<u8>>,
    ) -> IterState<'a> {
        loop {
            let out = if let Some(expanding) = &mut self.expanding {
                return match expanding.step(env, previous_out) {
                    IterState::Expand(cmd) => {
                        // inner command has to expand some parameters, yield back to
                        // the shell to execute it, and return `expanding` back to the
                        // state, so we feed the input back to it
                        IterState::Expand(cmd)
                    }
                    IterState::Ready(cmd) => {
                        // inner command is ready to be executed after expanding its,
                        // params, however it's _our_ expansion, so we'll rewrite its
                        // 'ready to an expand', but we won't replace it back into the
                        // state so the `previous_out` is written to our params
                        self.expanding = None;
                        IterState::Expand(cmd)
                    }
                };
            } else if let Some(arg) = previous_out.take() {
                // our `expanding` has completed, and we've received its output so lets
                // store it in our params
                Cow::Owned(arg)
            } else if let Some(arg) = self.command.next() {
                // traverse the command AST until we hit the next actionable part
                match arg {
                    ParsedPart::Break => {
                        // if we hit a break insert a new parameter to start writing into
                        if self.params.last().map_or(true, |v| !v.is_empty()) {
                            self.params.push(Cow::Borrowed(b""));
                        }
                        continue;
                    }
                    ParsedPart::String(data) => {
                        // push the string into our params
                        data
                    }
                    ParsedPart::Expansion(Expansion::Command(command)) => {
                        // command needs to be substituted so lets yield to it
                        self.expanding = Some(Box::new(Iter::new(command)));
                        continue;
                    }
                    ParsedPart::Expansion(Expansion::Variable(variable)) => {
                        // substitute environment variable in
                        env.get(&variable).cloned().unwrap_or(Cow::Borrowed(b""))
                    }
                    ParsedPart::Redirection(idx, target) => {
                        // store a stdio redirection
                        if let Some(out) = self.stdio_out.get_mut(usize::from(idx)) {
                            *out = target;
                        }
                        continue;
                    }
                }
            } else {
                // fully evaluated and ready to be executed
                return IterState::Ready(PartialCommand::new(
                    self.exec.clone(),
                    self.params.clone(),
                ));
            };

            if self.exec.is_none() {
                self.exec = Some(out);
            } else if let Some(lst) = self.params.last_mut() {
                lst.to_mut().extend_from_slice(&out);
            } else {
                self.params.push(out);
            }
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum ParsedPart<'a> {
    Break,
    String(Cow<'a, [u8]>),
    Expansion(Expansion<'a>),
    Redirection(u8, RedirectionTo<'a>),
}

impl ParsedPart<'_> {
    pub fn into_owned(self) -> ParsedPart<'static> {
        match self {
            ParsedPart::Break => ParsedPart::Break,
            ParsedPart::String(s) => ParsedPart::String(Cow::Owned(s.into_owned())),
            ParsedPart::Expansion(e) => ParsedPart::Expansion(e.into_owned()),
            ParsedPart::Redirection(s, e) => ParsedPart::Redirection(s, e.into_owned()),
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum RedirectionTo<'a> {
    Stdio(u8),
    File(Cow<'a, [u8]>),
}

impl RedirectionTo<'_> {
    pub fn into_owned(self) -> RedirectionTo<'static> {
        match self {
            RedirectionTo::Stdio(v) => RedirectionTo::Stdio(v),
            RedirectionTo::File(f) => RedirectionTo::File(Cow::Owned(f.into_owned())),
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum Expansion<'a> {
    Variable(Cow<'a, [u8]>),
    Command(Vec<ParsedPart<'a>>),
}

impl Expansion<'_> {
    pub fn into_owned(self) -> Expansion<'static> {
        match self {
            Expansion::Variable(v) => Expansion::Variable(Cow::Owned(v.into_owned())),
            Expansion::Command(c) => {
                Expansion::Command(c.into_iter().map(ParsedPart::into_owned).collect())
            }
        }
    }
}

/// Parses a single command (including substitutions), a command is delimited by a `;`, `|` or `>`
pub fn tokenize(s: &[u8]) -> IResult<&[u8], Vec<ParsedPart<'_>>> {
    fold_many0(parse_string_part, Vec::new, |mut acc, res| {
        acc.extend(res);
        acc
    })(s)
}

fn parse_string_part(s: &[u8]) -> IResult<&[u8], Vec<ParsedPart<'_>>> {
    if s.is_empty() {
        return context("empty input", fail)(s);
    }

    alt((
        parse_double_quoted,
        map(
            alt((
                parse_redirection,
                map(multispace1, |_| ParsedPart::Break),
                map(parse_single_quoted, |r| {
                    ParsedPart::String(Cow::Borrowed(r))
                }),
                map(parse_expansion, ParsedPart::Expansion),
                map(parse_unquoted, |r| ParsedPart::String(Cow::Owned(r))),
            )),
            |r| vec![r],
        ),
    ))(s)
}

fn parse_redirection(s: &[u8]) -> IResult<&[u8], ParsedPart<'_>> {
    let (s, from) = map_opt(digit0, atoi)(s)?;
    let (s, _) = char('>')(s)?;
    let (s, to) = alt((
        map(
            preceded(char('&'), map_opt(digit1, atoi)),
            RedirectionTo::Stdio,
        ),
        map(alphanumeric1, |f| RedirectionTo::File(Cow::Borrowed(f))),
    ))(s)?;

    Ok((s, ParsedPart::Redirection(from, to)))
}

fn parse_unquoted(s: &[u8]) -> IResult<&[u8], Vec<u8>> {
    escaped_transform(
        is_not("\\\n \"'$`|>&();"),
        '\\',
        alt((value(b"".as_slice(), char('\n')), take(1_u8))),
    )(s)
}

fn parse_single_quoted(s: &[u8]) -> IResult<&[u8], &[u8]> {
    // no special chars in single quoted, so we just need to read ahead
    // until the end quote
    delimited(char('\''), take_until("'"), char('\''))(s)
}

fn parse_double_quoted(s: &[u8]) -> IResult<&[u8], Vec<ParsedPart<'_>>> {
    let escaped = escaped_transform(
        is_not("\\\"$`"),
        '\\',
        alt((
            value(b"\"".as_slice(), char('"')),
            value(b"\n".as_slice(), char('n')),
            value(b"\t".as_slice(), char('t')),
            value(b"$".as_slice(), char('$')),
            value(b"`".as_slice(), char('`')),
            value(b"\\".as_slice(), char('\\')),
        )),
    );

    let take_part = alt((
        map(escaped, |r| ParsedPart::String(Cow::Owned(r))),
        map(parse_expansion, ParsedPart::Expansion),
    ));

    delimited(
        char('"'),
        map(many_till(take_part, peek(char('"'))), |(r, _)| r),
        char('"'),
    )(s)
}

fn parse_expansion(s: &[u8]) -> IResult<&[u8], Expansion<'_>> {
    let dollar_expansion = alt((
        map(tag("$"), |f| Expansion::Variable(Cow::Borrowed(f))),
        map(
            delimited(
                char('('),
                cut(context("tokenize", tokenize)),
                cut(context("end brace", char(')'))),
            ),
            Expansion::Command,
        ),
        map(take_while1(|c: u8| c.is_alphanum() || c == b'_'), |f| {
            Expansion::Variable(Cow::Borrowed(f))
        }),
        map(
            // TODO: this should deal with bash variable expansion operators
            //  like `-` which allows for a rhs default is a var is unset
            delimited(
                char('{'),
                take_until("}"),
                cut(context("end brace", char('}'))),
            ),
            |f| Expansion::Variable(Cow::Borrowed(f)),
        ),
    ));

    alt((
        preceded(char('$'), dollar_expansion),
        map(
            delimited(char('`'), context("tokenize", tokenize), char('`')),
            Expansion::Command,
        ),
    ))(s)
}

fn atoi(v: &[u8]) -> Option<u8> {
    if v.is_empty() {
        Some(0)
    } else {
        atoi::atoi(v)
    }
}

#[cfg(test)]
mod test {
    mod iter {
        use std::borrow::Cow;

        use crate::{
            command::PartialCommand,
            server::ConnectionState,
            subsystem::shell::parser::{tokenize, Iter, IterState},
        };

        #[test]
        fn single_nested() {
            let (rest, s) = tokenize(b"echo $(echo hello) world!").unwrap();
            assert!(rest.is_empty());

            let state = ConnectionState::mock();
            let mut command = Iter::new(s);

            // once we step we should be requested to execute `echo hello` for subbing
            let step = command.step(state.environment(), None);
            assert_eq!(
                step,
                IterState::Expand(PartialCommand::new(
                    Some(Cow::Borrowed(b"echo")),
                    vec![Cow::Borrowed(b"hello")]
                ))
            );

            // step again with the supposed output of the command we were requested to execute
            // and we should receive the final command to execute
            let step = command.step(state.environment(), Some(b"hello".to_vec()));
            assert_eq!(
                step,
                IterState::Ready(PartialCommand::new(
                    Some(Cow::Borrowed(b"echo")),
                    vec![Cow::Borrowed(b"hello"), Cow::Borrowed(b"world!")]
                ))
            );
        }

        #[test]
        fn multi_nested() {
            let (rest, s) = tokenize(b"echo $(echo hello `echo the whole`) world!").unwrap();
            assert!(rest.is_empty());

            let state = ConnectionState::mock();
            let mut command = Iter::new(s);

            // once we step we should be requested to execute `echo the whole` for subbing
            let step = command.step(state.environment(), None);
            assert_eq!(
                step,
                IterState::Expand(PartialCommand::new(
                    Some(Cow::Borrowed(b"echo")),
                    vec![Cow::Borrowed(b"the"), Cow::Borrowed(b"whole")]
                ))
            );

            // once we step we should be requested to execute `echo hello` for subbing
            let step = command.step(state.environment(), Some(b"the whole".to_vec()));
            assert_eq!(
                step,
                IterState::Expand(PartialCommand::new(
                    Some(Cow::Borrowed(b"echo")),
                    vec![Cow::Borrowed(b"hello"), Cow::Borrowed(b"the whole")]
                ))
            );

            // step again with the supposed output of the command we were requested to execute
            // and we should receive the final command to execute
            let step = command.step(state.environment(), Some(b"hello the whole".to_vec()));
            assert_eq!(
                step,
                IterState::Ready(PartialCommand::new(
                    Some(Cow::Borrowed(b"echo")),
                    vec![Cow::Borrowed(b"hello the whole"), Cow::Borrowed(b"world!")]
                ))
            );
        }
    }

    mod parse_command {
        use std::borrow::Cow;

        use crate::subsystem::shell::parser::{tokenize, Expansion, ParsedPart, RedirectionTo};

        #[test]
        fn messed_up() {
            let (rest, s) = tokenize(b"echo    ${HI}'this' \"is a \\t${TEST}\"using'$(complex string)>|' $(echo parsing) for the hell of it;fin").unwrap();
            assert_eq!(rest, b";fin");
            assert_eq!(
                s,
                vec![
                    ParsedPart::String(Cow::Borrowed(b"echo")),
                    ParsedPart::Break,
                    ParsedPart::Expansion(Expansion::Variable(Cow::Borrowed(b"HI"))),
                    ParsedPart::String(Cow::Borrowed(b"this")),
                    ParsedPart::Break,
                    ParsedPart::String(Cow::Borrowed(b"is a \t")),
                    ParsedPart::Expansion(Expansion::Variable(Cow::Borrowed(b"TEST"))),
                    ParsedPart::String(Cow::Borrowed(b"using")),
                    ParsedPart::String(Cow::Borrowed(b"$(complex string)>|")),
                    ParsedPart::Break,
                    ParsedPart::Expansion(Expansion::Command(vec![
                        ParsedPart::String(Cow::Borrowed(b"echo")),
                        ParsedPart::Break,
                        ParsedPart::String(Cow::Borrowed(b"parsing")),
                    ])),
                    ParsedPart::Break,
                    ParsedPart::String(Cow::Borrowed(b"for")),
                    ParsedPart::Break,
                    ParsedPart::String(Cow::Borrowed(b"the")),
                    ParsedPart::Break,
                    ParsedPart::String(Cow::Borrowed(b"hell")),
                    ParsedPart::Break,
                    ParsedPart::String(Cow::Borrowed(b"of")),
                    ParsedPart::Break,
                    ParsedPart::String(Cow::Borrowed(b"it")),
                ]
            );
        }

        #[test]
        fn parses_named_redirects() {
            let (rest, s) = tokenize(b"hello test 2>&1").unwrap();
            assert!(rest.is_empty(), "{}", String::from_utf8_lossy(rest));
            assert_eq!(
                s,
                vec![
                    ParsedPart::String(Cow::Borrowed(b"hello")),
                    ParsedPart::Break,
                    ParsedPart::String(Cow::Borrowed(b"test")),
                    ParsedPart::Break,
                    ParsedPart::Redirection(2, RedirectionTo::Stdio(1)),
                ]
            );
        }

        #[test]
        fn parses_unnamed_redirects() {
            let (rest, s) = tokenize(b"hello test >&1").unwrap();
            assert!(rest.is_empty(), "{}", String::from_utf8_lossy(rest));
            assert_eq!(
                s,
                vec![
                    ParsedPart::String(Cow::Borrowed(b"hello")),
                    ParsedPart::Break,
                    ParsedPart::String(Cow::Borrowed(b"test")),
                    ParsedPart::Break,
                    ParsedPart::Redirection(0, RedirectionTo::Stdio(1)),
                ]
            );
        }
    }

    mod parse_expansion {
        use std::borrow::Cow;

        use crate::subsystem::shell::parser::{parse_expansion, Expansion, ParsedPart};

        #[test]
        fn double_dollar() {
            let (rest, s) = parse_expansion(b"$$a").unwrap();
            assert_eq!(rest, b"a");
            assert_eq!(s, Expansion::Variable(Cow::Borrowed(b"$")));
        }

        #[test]
        fn variable() {
            let (rest, s) = parse_expansion(b"$HELLO_WORLD").unwrap();
            assert!(rest.is_empty());
            assert_eq!(s, Expansion::Variable(Cow::Borrowed(b"HELLO_WORLD")));
        }

        #[test]
        fn variable_split() {
            let (rest, s) = parse_expansion(b"$HELLO-WORLD").unwrap();
            assert_eq!(rest, b"-WORLD");
            assert_eq!(s, Expansion::Variable(Cow::Borrowed(b"HELLO")));
        }

        #[test]
        fn braced_variable() {
            let (rest, s) = parse_expansion(b"${helloworld}").unwrap();
            assert!(rest.is_empty());
            assert_eq!(s, Expansion::Variable(Cow::Borrowed(b"helloworld")));
        }

        #[test]
        fn not_expansion() {
            parse_expansion(b"NOT_VARIABLE").expect_err("not variable");
        }

        #[test]
        fn nested() {
            let (rest, s) = parse_expansion(b"$(\'echo\' \'hello\')").unwrap();
            assert!(rest.is_empty(), "{rest:?}");
            assert_eq!(
                s,
                Expansion::Command(vec![
                    ParsedPart::String(Cow::Borrowed(b"echo")),
                    ParsedPart::Break,
                    ParsedPart::String(Cow::Borrowed(b"hello")),
                ])
            );
        }
    }

    mod parse_unquoted {
        use crate::subsystem::shell::parser::parse_unquoted;

        #[test]
        fn escape() {
            let (rest, s) =
                parse_unquoted(b"hello\\ \\world\\ \\thi\\ns\\ is\\ a\\ \\$test\\\n! dontparse")
                    .unwrap();
            assert_eq!(rest, b" dontparse", "{}", String::from_utf8_lossy(rest));
            assert_eq!(
                s,
                b"hello world thins is a $test!".to_vec(),
                "{}",
                String::from_utf8_lossy(&s)
            );
        }
    }

    mod parse_single_quoted {
        use crate::subsystem::shell::parser::parse_single_quoted;

        #[test]
        fn multi_quote() {
            let (rest, s) = parse_single_quoted(b"'hello''world'").unwrap();
            assert_eq!(rest, b"'world'");
            assert_eq!(s, b"hello");
        }
    }

    mod parse_double_quoted {
        use std::borrow::Cow;

        use crate::subsystem::shell::parser::{parse_double_quoted, Expansion, ParsedPart};

        #[test]
        fn with_expansion() {
            let (rest, s) = parse_double_quoted(b"\"hello world $('cat' 'test') test\"").unwrap();
            assert!(rest.is_empty());
            assert_eq!(
                s,
                vec![
                    ParsedPart::String(Cow::Borrowed(b"hello world ")),
                    ParsedPart::Expansion(Expansion::Command(vec![
                        ParsedPart::String(Cow::Borrowed(b"cat")),
                        ParsedPart::Break,
                        ParsedPart::String(Cow::Borrowed(b"test")),
                    ])),
                    ParsedPart::String(Cow::Borrowed(b" test")),
                ]
            );
        }

        #[test]
        fn with_expansion_escape() {
            let (rest, s) = parse_double_quoted(b"\"hello world \\$('cat' 'test') test\"").unwrap();
            assert!(rest.is_empty());
            assert_eq!(
                s,
                vec![ParsedPart::String(Cow::Borrowed(
                    b"hello world $('cat' 'test') test"
                ))]
            );
        }

        #[test]
        fn with_escape_code() {
            let (rest, s) = parse_double_quoted(b"\"hi\\nworld\"").unwrap();
            assert!(rest.is_empty());
            assert_eq!(s, vec![ParsedPart::String(Cow::Borrowed(b"hi\nworld"))]);
        }
    }
}
