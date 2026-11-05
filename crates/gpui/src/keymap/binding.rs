// Modified by Mesa Hills Research, as docs/upstream.md in mesa-hills-research/gpui describes.
use std::rc::Rc;

use crate::{
    Action, AsKeystroke, DummyKeyboardMapper, InvalidKeystrokeError, KeyBindingContextPredicate,
    KeybindingKeystroke, Keystroke, PlatformKeyboardMapper, SharedString,
};
use smallvec::SmallVec;

/// A keybinding and its associated metadata, from the keymap.
pub struct KeyBinding {
    pub(crate) action: Box<dyn Action>,
    pub(crate) keystrokes: SmallVec<[KeybindingKeystroke; 2]>,
    pub(crate) context_predicate: Option<Rc<KeyBindingContextPredicate>>,
    pub(crate) meta: Option<KeyBindingMetaIndex>,
    /// The json input string used when building the keybinding, if any
    pub(crate) action_input: Option<SharedString>,
}

impl Clone for KeyBinding {
    fn clone(&self) -> Self {
        KeyBinding {
            action: self.action.boxed_clone(),
            keystrokes: self.keystrokes.clone(),
            context_predicate: self.context_predicate.clone(),
            meta: self.meta,
            action_input: self.action_input.clone(),
        }
    }
}

impl KeyBinding {
    /// Constructs a static keybinding with explicit modifiers. Panics on parse error.
    ///
    /// Use [`Self::load`] for dynamic input or to resolve `secondary-` using the host OS.
    ///
    /// `keystrokes` holds one or more keystrokes separated by spaces, each written as
    /// [`Keystroke::parse`] describes.
    ///
    /// A chord that types a symbol can be written as the symbol (`ctrl-|`) or with Shift on its
    /// key (`ctrl-shift-\`). Platforms send Ctrl+Shift+\ as `ctrl-|`, and both spellings match
    /// it. The binding shows as written, so `ctrl-shift-\` reads as Ctrl+Shift+\ in a menu.
    /// Older versions of GPUI match only the symbol, so an app that supports them writes that.
    /// `null` and `Unbind` remove a binding in either spelling, in dispatch and in the shortcut
    /// a menu shows.
    ///
    /// Shift on a key means the symbol a US keyboard types. `ctrl-shift-2` is Ctrl+@, which a UK
    /// keyboard types with Shift+', and the chord on its Shift+2 types `"` and needs `ctrl-"`.
    /// Layouts that type digits with Shift, such as a French one, send the digit, so
    /// `ctrl-shift-1` written for `new` never matches there.
    ///
    /// [`KeyBinding::load`] with [`App::keyboard_mapper`](crate::App::keyboard_mapper) reads the
    /// active layout instead on Windows and Linux, so `ctrl-shift-2` is Ctrl+Shift+2 on a UK
    /// keyboard and `ctrl-shift-1` matches on a French one. On Linux a key that types a non-ASCII
    /// symbol (`£`, `§`, `ř`) keeps the US reading, so `ctrl-shift-3` still reads as `ctrl-#`
    /// there. A binding keeps the keys it was loaded with, so an app loads its keymap again from
    /// [`App::on_keyboard_layout_change`](crate::App::on_keyboard_layout_change).
    pub fn new<A: Action>(keystrokes: &'static str, action: A, context: Option<&str>) -> Self {
        let context_predicate =
            context.map(|context| KeyBindingContextPredicate::parse(context).unwrap().into());
        Self::load_internal(
            keystrokes,
            Box::new(action),
            context_predicate,
            false,
            None,
            &DummyKeyboardMapper,
            Keystroke::parse,
        )
        .unwrap()
    }

    /// Load a keybinding from the given raw data.
    pub fn load(
        keystrokes: &str,
        action: Box<dyn Action>,
        context_predicate: Option<Rc<KeyBindingContextPredicate>>,
        use_key_equivalents: bool,
        action_input: Option<SharedString>,
        cx: &crate::App,
    ) -> std::result::Result<Self, InvalidKeystrokeError> {
        Self::load_internal(
            keystrokes,
            action,
            context_predicate,
            use_key_equivalents,
            action_input,
            cx.keyboard_mapper().as_ref(),
            |source| Keystroke::parse_with_os(source, cx.platform_info().os),
        )
    }

    fn load_internal<'a>(
        keystrokes: &'a str,
        action: Box<dyn Action>,
        context_predicate: Option<Rc<KeyBindingContextPredicate>>,
        use_key_equivalents: bool,
        action_input: Option<SharedString>,
        keyboard_mapper: &dyn PlatformKeyboardMapper,
        parse_keystroke: impl Fn(&'a str) -> std::result::Result<Keystroke, InvalidKeystrokeError>,
    ) -> std::result::Result<Self, InvalidKeystrokeError> {
        let keystrokes: SmallVec<[KeybindingKeystroke; 2]> = keystrokes
            .split_whitespace()
            .map(|source| {
                let keystroke = parse_keystroke(source)?;
                Ok(KeybindingKeystroke::new_with_mapper(
                    keystroke,
                    use_key_equivalents,
                    keyboard_mapper,
                ))
            })
            .collect::<std::result::Result<_, _>>()?;

        Ok(Self {
            keystrokes,
            action,
            context_predicate,
            meta: None,
            action_input,
        })
    }

    /// Set the metadata for this binding.
    pub fn with_meta(mut self, meta: KeyBindingMetaIndex) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Set the metadata for this binding.
    pub fn set_meta(&mut self, meta: KeyBindingMetaIndex) {
        self.meta = Some(meta);
    }

    /// Check if the given keystrokes match this binding.
    pub fn match_keystrokes(&self, typed: &[impl AsKeystroke]) -> Option<bool> {
        if self.keystrokes.len() < typed.len() {
            return None;
        }

        for (target, typed) in self.keystrokes.iter().zip(typed.iter()) {
            if !typed.as_keystroke().should_match(target) {
                return None;
            }
        }

        Some(self.keystrokes.len() > typed.len())
    }

    /// Get the keystrokes associated with this binding
    pub fn keystrokes(&self) -> &[KeybindingKeystroke] {
        self.keystrokes.as_slice()
    }

    /// Get the action associated with this binding
    pub fn action(&self) -> &dyn Action {
        self.action.as_ref()
    }

    /// Get the predicate used to match this binding
    pub fn predicate(&self) -> Option<Rc<KeyBindingContextPredicate>> {
        self.context_predicate.as_ref().map(|rc| rc.clone())
    }

    /// Get the metadata for this binding
    pub fn meta(&self) -> Option<KeyBindingMetaIndex> {
        self.meta
    }

    /// Get the action input associated with the action for this binding
    pub fn action_input(&self) -> Option<SharedString> {
        self.action_input.clone()
    }
}

impl std::fmt::Debug for KeyBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyBinding")
            .field("keystrokes", &self.keystrokes)
            .field("context_predicate", &self.context_predicate)
            .field("action", &self.action.name())
            .finish()
    }
}

/// A unique identifier for retrieval of metadata associated with a key binding.
/// Intended to be used as an index or key into a user-defined store of metadata
/// associated with the binding, such as the source of the binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBindingMetaIndex(pub u32);
