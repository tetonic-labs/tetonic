//! Remove test-only Rust items using parsed spans, never quote/brace guesses.
use syn::{spanned::Spanned, visit::Visit, Attribute, Item};

fn test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Path>()
                .is_ok_and(|path| path.is_ident("test"))
    })
}

struct TestItems {
    spans: Vec<proc_macro2::Span>,
}
impl<'ast> Visit<'ast> for TestItems {
    fn visit_item(&mut self, item: &'ast Item) {
        let attrs = match item {
            Item::Const(v) => &v.attrs,
            Item::Enum(v) => &v.attrs,
            Item::ExternCrate(v) => &v.attrs,
            Item::Fn(v) => &v.attrs,
            Item::ForeignMod(v) => &v.attrs,
            Item::Impl(v) => &v.attrs,
            Item::Macro(v) => &v.attrs,
            Item::Mod(v) => &v.attrs,
            Item::Static(v) => &v.attrs,
            Item::Struct(v) => &v.attrs,
            Item::Trait(v) => &v.attrs,
            Item::TraitAlias(v) => &v.attrs,
            Item::Type(v) => &v.attrs,
            Item::Union(v) => &v.attrs,
            Item::Use(v) => &v.attrs,
            _ => {
                syn::visit::visit_item(self, item);
                return;
            }
        };
        if test_only(attrs) {
            self.spans.push(item.span());
        } else {
            syn::visit::visit_item(self, item);
        }
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if test_only(&item.attrs) {
            self.spans.push(item.span());
        } else {
            syn::visit::visit_impl_item_fn(self, item);
        }
    }
}

pub fn strip_cfg_test_blocks(src: &str) -> String {
    // Invalid or fragmentary planted fixtures remain visible to detectors.
    let Ok(file) = syn::parse_file(src) else {
        return src.to_owned();
    };
    let mut visitor = TestItems { spans: Vec::new() };
    visitor.visit_file(&file);
    let mut starts = vec![0];
    starts.extend(
        src.bytes()
            .enumerate()
            .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
    );
    let offset = |point: proc_macro2::LineColumn| -> usize {
        let start = starts[point.line - 1];
        start
            + src[start..]
                .char_indices()
                .nth(point.column)
                .map_or(src.len() - start, |(i, _)| i)
    };
    let mut bytes = src.as_bytes().to_vec();
    for span in visitor.spans {
        let start = offset(span.start());
        let end = offset(span.end());
        for b in &mut bytes[start..end] {
            if *b != b'\n' && *b != b'\r' {
                *b = b' ';
            }
        }
    }
    String::from_utf8(bytes).expect("only complete Rust items were replaced")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifetimes_raw_strings_and_nested_comments_preserve_production() {
        let src = r###"#[cfg(test)] mod tests {
            fn f(x: &'static str) {}
            const S: &str = r#" braces } and quote " "#;
            /* outer /* nested } */ tail */
        }
        fn production_mutant() {}"###;
        let out = strip_cfg_test_blocks(src);
        assert!(out.contains("fn production_mutant()"));
        assert!(!out.contains("fn f("));
    }
    #[test]
    fn text_that_looks_like_attribute_is_not_an_attribute() {
        let src = r##"const S: &str = "#[cfg(test)]"; fn production() {}"##;
        assert_eq!(strip_cfg_test_blocks(src), src);
    }
}
