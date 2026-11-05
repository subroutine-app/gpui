// Modified for gpui-pre (snapshot of zed@f816e1f): this module is added by the script.
// Injected by gpui-kit's script/bump-gpui.ts.
// Modified by Mesa Hills Research, as docs/upstream.md in mesa-hills-research/gpui describes.
use proc_macro::{Group, Ident, Literal, Punct, Spacing, TokenStream, TokenTree};
use proc_macro_crate::{crate_name, FoundCrate};

#[derive(Debug, PartialEq)]
enum FacadePath {
    Itself,
    Name(String),
}

// The packages of the facade crate, in the order they are tried. `gpui-kit` is the name it had
// before it was published as `mhr-gpui-kit`, and apps that still depend on it keep working.
const FACADE_PACKAGES: [&str; 2] = ["mhr-gpui-kit", "gpui-kit"];

pub(crate) fn rewrite(stream: TokenStream) -> TokenStream {
    let facade = find_facade();
    rewrite_stream(stream, facade.as_ref())
}

fn find_facade() -> Option<FacadePath> {
    FACADE_PACKAGES.iter().find_map(|package| match crate_name(package) {
        // The library of `mhr-gpui-kit` is `gpui_kit`, which is what a dependency on it without a
        // rename is called in code. A rename (`gpui-kit = { package = "mhr-gpui-kit" }`) keeps its key.
        Ok(FoundCrate::Name(name)) if name == "mhr_gpui_kit" => Some(FacadePath::Name("gpui_kit".into())),
        Ok(FoundCrate::Name(name)) => Some(FacadePath::Name(name)),
        Ok(FoundCrate::Itself) => Some(FacadePath::Itself),
        Err(_) => None,
    })
}

fn rewrite_stream(stream: TokenStream, facade: Option<&FacadePath>) -> TokenStream {
    let tokens: Vec<_> = stream.into_iter().collect();
    let mut output = TokenStream::new();
    for (index, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => {
                let mut rewritten = Group::new(group.delimiter(), rewrite_stream(group.stream(), facade));
                rewritten.set_span(group.span());
                output.extend([TokenTree::Group(rewritten)]);
            }
            TokenTree::Ident(ident)
                if is_path_head(&tokens, index)
                    && (ident.to_string() == "gpui" || ident.to_string() == "gpui_platform") =>
            {
                append_path_head(&mut output, ident, facade);
            }
            TokenTree::Literal(literal) => {
                output.extend([TokenTree::Literal(rewrite_literal(literal, facade))]);
            }
            token => output.extend([token.clone()]),
        }
    }
    output
}

fn is_path_head(tokens: &[TokenTree], index: usize) -> bool {
    matches!(tokens.get(index + 1), Some(TokenTree::Punct(first)) if first.as_char() == ':')
        && matches!(tokens.get(index + 2), Some(TokenTree::Punct(second)) if second.as_char() == ':')
}

fn append_path_head(output: &mut TokenStream, original: &Ident, facade: Option<&FacadePath>) {
    let name = match facade {
        Some(FacadePath::Itself) => "crate",
        Some(FacadePath::Name(name)) => name,
        None => {
            output.extend([TokenTree::Ident(original.clone())]);
            return;
        }
    };
    output.extend([TokenTree::Ident(Ident::new(name, original.span()))]);
    if original.to_string() == "gpui_platform" {
        output.extend([
            TokenTree::Punct(Punct::new(':', Spacing::Joint)),
            TokenTree::Punct(Punct::new(':', Spacing::Alone)),
            TokenTree::Ident(Ident::new("platform", original.span())),
        ]);
    }
}

fn rewrite_literal(literal: &Literal, facade: Option<&FacadePath>) -> Literal {
    let (gpui, platform) = match facade {
        Some(FacadePath::Itself) => ("crate::".into(), "crate::platform::".into()),
        Some(FacadePath::Name(name)) =>
            (format!("::{name}::"), format!("::{name}::platform::")),
        None => return literal.clone(),
    };
    let text = literal.to_string();
    let rewritten = text
        .replace("::gpui_platform::", &platform)
        .replace("::gpui::", &gpui);
    if rewritten == text { return literal.clone() }
    let Ok(parsed) = rewritten.parse::<TokenStream>() else { return literal.clone() };
    let mut tokens = parsed.into_iter();
    match (tokens.next(), tokens.next()) {
        (Some(TokenTree::Literal(literal)), None) => literal,
        _ => literal.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Looks up the facade from the `Cargo.toml` of a made-up package, as proc-macro-crate reads it
    // while that package compiles. `integration_test` is how cargo marks the build of a tests/ file.
    fn facade_of(manifest: &str, integration_test: bool) -> Option<FacadePath> {
        // Each call gets a folder of its own, because proc-macro-crate caches a manifest's answer by
        // its path and modification time, and a rewrite within the same tick of a coarse file clock
        // (as on Windows) would read as unchanged.
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("gpui-facade-{}-{call}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
        // Safe, since this is the only test of the crate and nothing else reads the environment.
        unsafe {
            std::env::set_var("CARGO_MANIFEST_DIR", &dir);
            match integration_test {
                true => std::env::set_var("CARGO_TARGET_TMPDIR", &dir),
                false => std::env::remove_var("CARGO_TARGET_TMPDIR"),
            }
        }
        let facade = find_facade();
        std::fs::remove_dir_all(&dir).unwrap();
        facade
    }

    #[test]
    fn finds_the_kit_under_either_package_name() {
        let app = |dependency: &str| {
            format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\n{dependency}\n")
        };
        let name = |name: &str| Some(FacadePath::Name(name.into()));

        // An app that depends only on mhr-gpui-kit imports its library, gpui_kit.
        assert_eq!(facade_of(&app("mhr-gpui-kit = \"0.7\""), false), name("gpui_kit"));
        // A rename gives the name it is imported as.
        let renamed = "gpui-kit = { package = \"mhr-gpui-kit\", version = \"0.7\" }";
        assert_eq!(facade_of(&app(renamed), false), name("gpui_kit"));
        let renamed = "kit = { package = \"mhr-gpui-kit\", version = \"0.7\" }";
        assert_eq!(facade_of(&app(renamed), false), name("kit"));
        // The package before it was published as mhr-gpui-kit.
        assert_eq!(facade_of(&app("gpui-kit = \"0.7\""), false), name("gpui_kit"));
        // The kit itself uses `crate`, except in its tests/, which import it by its library name.
        let kit = "[package]\nname = \"mhr-gpui-kit\"\nversion = \"0.7.0\"\n";
        assert_eq!(facade_of(kit, false), Some(FacadePath::Itself));
        assert_eq!(facade_of(kit, true), name("gpui_kit"));
        // An app with gpui alone keeps `gpui::`.
        assert_eq!(facade_of(&app("gpui = { package = \"mhr-gpui\", version = \"0.1\" }"), false), None);
    }
}
