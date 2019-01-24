use super::tokens::*;
use super::ast::*;

use super::super::source::{Source, SrcMgr, DiagMgr, Severity, Span};

use std::rc::Rc;
use std::collections::VecDeque;
use std::collections::HashMap;

pub fn pp<'a>(mgr: &'a SrcMgr, diag: &'a DiagMgr, src: &Rc<Source>) -> VecDeque<Token> {
    Preprocessor::new(mgr, diag).all(src)
}

struct Preprocessor<'a> {
    mgr: &'a SrcMgr,
    diag: &'a DiagMgr,
    stacks: Vec<VecDeque<Token>>,
    macros: HashMap<String, (Span, VecDeque<Token>)>,
    // A branch stack indicating whether previous branch is taken and whether an else is encountered
    branch_stack: Vec<(bool, bool)>,
}

impl<'a> Preprocessor<'a> {
    fn new(mgr: &'a SrcMgr, diag: &'a DiagMgr) -> Preprocessor<'a> {
        Preprocessor {
            mgr,
            diag,
            stacks: Vec::new(),
            macros: HashMap::new(),
            branch_stack: Vec::new(),
        }
    }

    fn peek_raw(&mut self) -> Option<&Token> {
        self.stacks.last_mut().unwrap().front()
    }

    /// Retrieve next raw, unprocessed token
    fn next_raw(&mut self) -> Option<Token> {
        loop {
            match self.stacks.last_mut() {
                None => return None,
                Some(v) => match v.pop_front() {
                    None => (),
                    Some(v) => return Some(v),
                }
            }
            self.stacks.pop();
        }
    }

    fn pushback_raw(&mut self, tok: Token) {
        match self.stacks.last_mut() {
            Some(v) => return v.push_front(tok),
            None => (),
        }
        self.stacks.push({
            let mut list = VecDeque::new();
            list.push_back(tok);
            list
        });
    }

    /// Check if a name is one of built-in directive.
    fn is_directive(name: &str) -> bool {
        match name {
            "resetall" |
            "include" |
            "define" |
            "undef" |
            "undefineall" |
            "ifdef" |
            "else" |
            "elsif" |
            "endif" |
            "ifndef" |
            "timescale" |
            "default_nettype" |
            "unconnected_drive" |
            "nounconnected_drive" |
            "celldefine" |
            "endcelldefine" |
            "pragma" |
            "line" |
            "__FILE__" |
            "__LINE__" |
            "begin_keywords" |
            "end_keywords" => true,
            _ => false,
        }
    }

    fn process(&mut self) -> Option<Token> {
        let mut after_newline = false;
        loop {
            let (name, span) = match self.next_raw() {
                // Found a directive
                Some(Spanned{value: TokenKind::Directive(name), span}) => (name, span),
                // Newline token, set after_newline and continue
                Some(Spanned{value: TokenKind::NewLine, ..}) |
                Some(Spanned{value: TokenKind::LineComment, ..}) => {
                    after_newline = true;
                    continue;
                }
                // Not a directive, just return as-is
                v => return v,
            };

            match name.as_ref() {
                "resetall" => {
                    self.diag.report_span(Severity::Warning, "compiler directive not yet supported", span);
                }
                "include" => {
                    if !after_newline {
                        self.diag.report_error("`include must be on its own line", span);
                    }
                    self.diag.report_span(Severity::Warning, "compiler directive not yet supported", span);
                }
                "define" => self.parse_define(span),
                "undef" |
                "undefineall" => {
                    self.diag.report_span(Severity::Warning, "compiler directive not yet supported", span);
                }
                "ifdef" => self.parse_ifdef(span, true),
                "ifndef" => self.parse_ifdef(span, false),
                "else" => self.parse_else(span),
                "elsif" => self.parse_elsif(span),
                "endif" => self.parse_endif(span),
                "timescale" |
                "default_nettype" |
                "unconnected_drive" |
                "nounconnected_drive" |
                "celldefine" |
                "endcelldefine" |
                "pragma" |
                "line" |
                "__FILE__" |
                "__LINE__" |
                "begin_keywords" |
                "end_keywords" => {
                    self.diag.report_span(Severity::Warning, "compiler directive not yet supported", span);
                }
                _ => {
                    // TODO: Replace macro within macro and handle `", ``, etc
                    match self.macros.get(&name) {
                        None => {
                            self.diag.report_error(
                                format!("cannot find macro {}", name),
                                span
                            );
                        }
                        Some((_, list)) => {
                            let mut newlist = list.clone();
                            for tok in &mut newlist {
                                // Replace all token spans in replacement list
                                tok.span = span;
                            }
                            self.stacks.push(newlist)
                        }
                    }
                }
            }

            after_newline = false;
        }
    }

    /// Read all tokens until the next newline (new line will be consumed but not returned)
    fn read_until_newline(&mut self) -> VecDeque<Token> {
        // Keep adding tokens to the list and stop when eof or eol is encountered.
        let mut list = VecDeque::new();
        loop {
            let tok = match self.next_raw() {
                None => break,
                Some(v) => v,
            };
            match tok.value {
                TokenKind::NewLine |
                TokenKind::LineComment => break,
                _ => (),
            }
            list.push_back(tok);
        }
        list
    }

    /// Read an identifier
    fn expect_id(&mut self) -> Option<Spanned<String>> {
        match self.next_raw() {
            Some(Spanned{value: TokenKind::Id(id), span}) => Some(Spanned::new(id, span)),
            _ => None,
        }
    }

    /// Parse a macro definition
    /// The span here is only for diagnostic purposes.
    fn parse_define(&mut self, span: Span) {
        // Read the name of this macro
        let (name, span) = match self.expect_id() {
            Some(v) => (v.value, v.span),
            None => {
                self.diag.report_error("expected identifier name after `define", span);
                // Error recovery: Discard until newline
                self.read_until_newline();
                return;
            }
        };

        if Self::is_directive(&name) {
            self.diag.report_error("directive name cannot be used as macro names", span);
            // Error recovery: Discard until newline
            self.read_until_newline();
            return;
        }

        // Check if this macro is function-like.
        let paren = match self.peek_raw().unwrap() {
            // If this is a parenthesis that immediately follows the name
            Spanned{value: TokenKind::OpenDelim(Delim::Paren), span: p_span} if p_span.start == span.end => true,
            _ => false,
        };

        if paren {
            // Discard the parenthesis
            self.next_raw();
            self.diag.report_span(Severity::Warning, "function-like macros not yet supported", span);
            // TODO: Parse formal args
            return;
        }

        let list = self.read_until_newline();

        // Insert it to the global definitions list and report error for duplicate definition
        if let Some((old_span, _)) = self.macros.insert(name, (span, list)) {
            self.diag.report_error("duplicate macro definitions", span);
            self.diag.report_span(Severity::Remark, "previous declared here", old_span);
        }
    }

    /// Parse an ifdef directive
    fn parse_ifdef(&mut self, span: Span, cond: bool) {
        // If this block is nested within a untaken branch, just skip everything
        if let Some((false, _)) = self.branch_stack.last() {
            self.branch_stack.push((false, false));
            return self.skip_tokens();
        }

        // Read the name of this macro
        let name = match self.expect_id() {
            Some(v) => v.value,
            None => {
                self.diag.report_error("expected identifier name after `ifdef or `ifndef", span);
                // Error recovery: Return a non-existing name, thus treating as untaken
                "".to_owned()
            }
        };

        let taken = self.macros.contains_key(&name) == cond;
        self.branch_stack.push((taken, false));
        if !taken {
            self.skip_tokens();
        }
    }

    /// Parse an elsif directive
    fn parse_elsif(&mut self, span: Span) {
        match self.branch_stack.last() {
            None => {
                // An elsif without corresponding if
                self.diag.report_error("`elsif without matching `ifdef or `ifndef", span);
                return;
            }
            Some((_, true)) => {
                // There is already an else
                self.diag.report_error("`elsif after an `else", span);
                // Error recovery: skip
                self.skip_tokens();
                return;
            }
            Some((true, false)) => {
                // Already taken, skip this branch
                self.skip_tokens();
                return;
            }
            _ => {
                // Remove the previous result
                self.branch_stack.pop();
            }
        }

        // If this block is nested within a untaken branch, just skip everything
        if let Some((false, _)) = self.branch_stack.last() {
            self.branch_stack.push((false, false));
            return self.skip_tokens();
        }

        // Read the name of this macro
        let name = match self.expect_id() {
            Some(v) => v.value,
            None => {
                self.diag.report_error("expected identifier name after `ifdef or `ifndef", span);
                // Error recovery: Return a non-existing name, thus treating as untaken
                "".to_owned()
            }
        };

        let taken = self.macros.contains_key(&name);
        self.branch_stack.push((taken, false));
        if !taken {
            self.skip_tokens();
        }
    }

    /// Parse an else directive
    fn parse_else(&mut self, span: Span) {
        match self.branch_stack.last() {
            None => {
                // An elsif without corresponding if
                self.diag.report_error("`else without matching `ifdef or `ifndef", span);
                return;
            }
            Some((_, true)) => {
                // There is already an else
                self.diag.report_error("`else after an `else", span);
                // Error recovery: skip
                self.skip_tokens();
                return;
            }
            Some((true, false)) => {
                // Already taken, skip this branch
                self.skip_tokens();
                return;
            }
            _ => {
                // Remove the previous result
                self.branch_stack.pop();
            }
        }

        // If this block is nested within a untaken branch, just skip everything
        if let Some((false, _)) = self.branch_stack.last() {
            self.branch_stack.push((false, true));
            return self.skip_tokens();
        }

        self.branch_stack.push((true, true));
    }

    /// Parse an endif directive
    fn parse_endif(&mut self, span: Span) {
        match self.branch_stack.pop() {
            None => {
                // An endif without corresponding if
                self.diag.report_error("`endif without matching `ifdef or `ifndef", span);
            }
            Some(_) => {
                // If previous branch is not taken, then we still need to ignore things after this `endif
                if let Some((false, _)) = self.branch_stack.last() {
                    self.skip_tokens()
                }
            }
        }
    }

    /// Skip tokens until next branching directive or eof
    fn skip_tokens(&mut self) {
        loop {
            let token = match self.next_raw() {
                // EOF
                None => return,
                Some(v) => v,
            };

            match token.value {
                TokenKind::Directive(ref name) => {
                    match name.as_ref() {
                        // Branching directive
                        "ifdef" |
                        "ifndef" |
                        "else" |
                        "elsif" |
                        "endif" => (),
                        _ => continue,
                    }
                }
                // Other token, skip
                _ => continue,
            }

            // Now push back the token and return
            self.pushback_raw(token);
            return;
        }
    }

    fn all(&mut self, src: &Rc<Source>) -> VecDeque<Token> {
        self.stacks.push(super::lex(self.mgr, self.diag, src));
        let mut vec = VecDeque::new();
        loop {
            match self.process() {
                None => break,
                Some(v) => match v.value {
                    TokenKind::NewLine |
                    TokenKind::LineComment => (),
                    _ => vec.push_back(v),
                }
            }
        }
        vec
    }
}