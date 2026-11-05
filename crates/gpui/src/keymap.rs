// Modified by Mesa Hills Research, as docs/upstream.md in mesa-hills-research/gpui describes.
mod binding;
mod context;

pub use binding::*;
pub use context::*;

use crate::{Action, AsKeystroke, Keystroke, Modifiers, Unbind, is_no_action, is_unbind};
use collections::{HashSet, TypeIdHashMap};
use smallvec::SmallVec;

/// An opaque identifier of which version of the keymap is currently active.
/// The keymap's version is changed whenever bindings are added or removed.
#[derive(Copy, Clone, Eq, PartialEq, Default)]
pub struct KeymapVersion(usize);

/// A collection of key bindings for the user's application.
#[derive(Default)]
pub struct Keymap {
    bindings: Vec<KeyBinding>,
    binding_indices_by_action_id: TypeIdHashMap<SmallVec<[usize; 3]>>,
    disabled_binding_indices: Vec<usize>,
    version: KeymapVersion,
}

/// Index of a binding within a keymap.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct BindingIndex(usize);

fn disabled_binding_matches_context(disabled_binding: &KeyBinding, binding: &KeyBinding) -> bool {
    match (
        &disabled_binding.context_predicate,
        &binding.context_predicate,
    ) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(disabled_predicate), Some(predicate)) => disabled_predicate.is_superset(predicate),
    }
}

/// A binding's keystrokes with Shift folded into symbol keys, as matching reads them, so
/// `ctrl-shift-\` and `ctrl-|` compare and hash alike.
fn folded_keystrokes(binding: &KeyBinding) -> SmallVec<[(Modifiers, &str, Option<&str>); 2]> {
    binding
        .keystrokes
        .iter()
        .map(|keystroke| keystroke.inner().folded())
        .collect()
}

fn binding_is_unbound(disabled_binding: &KeyBinding, binding: &KeyBinding) -> bool {
    folded_keystrokes(disabled_binding) == folded_keystrokes(binding)
        && disabled_binding
            .action()
            .as_any()
            .downcast_ref::<Unbind>()
            .is_some_and(|unbind| unbind.0.as_ref() == binding.action.name())
}

impl Keymap {
    /// Create a new keymap with the given bindings.
    pub fn new(bindings: Vec<KeyBinding>) -> Self {
        let mut this = Self::default();
        this.add_bindings(bindings);
        this
    }

    /// Get the current version of the keymap.
    pub fn version(&self) -> KeymapVersion {
        self.version
    }

    /// Add more bindings to the keymap.
    pub fn add_bindings<T: IntoIterator<Item = KeyBinding>>(&mut self, bindings: T) {
        for binding in bindings {
            let action_id = binding.action().as_any().type_id();
            if is_no_action(&*binding.action) || is_unbind(&*binding.action) {
                self.disabled_binding_indices.push(self.bindings.len());
            } else {
                self.binding_indices_by_action_id
                    .entry(action_id)
                    .or_default()
                    .push(self.bindings.len());
            }
            self.bindings.push(binding);
        }

        self.version.0 += 1;
    }

    /// Reset this keymap to its initial state.
    pub fn clear(&mut self) {
        self.bindings.clear();
        self.binding_indices_by_action_id.clear();
        self.disabled_binding_indices.clear();
        self.version.0 += 1;
    }

    /// Iterate over all bindings, in the order they were added.
    pub fn bindings(&self) -> impl DoubleEndedIterator<Item = &KeyBinding> + ExactSizeIterator {
        self.bindings.iter()
    }

    /// Iterate over all bindings for the given action, in the order they were added. For display,
    /// the last binding should take precedence.
    pub fn bindings_for_action<'a>(
        &'a self,
        action: &'a dyn Action,
    ) -> impl 'a + DoubleEndedIterator<Item = &'a KeyBinding> {
        let action_id = action.type_id();
        let binding_indices = self
            .binding_indices_by_action_id
            .get(&action_id)
            .map_or(&[] as _, SmallVec::as_slice)
            .iter();

        binding_indices.filter_map(|ix| {
            let binding = &self.bindings[*ix];
            if !binding.action().partial_eq(action) {
                return None;
            }

            let keystrokes = folded_keystrokes(binding);
            for disabled_ix in &self.disabled_binding_indices {
                if disabled_ix > ix {
                    let disabled_binding = &self.bindings[*disabled_ix];
                    if folded_keystrokes(disabled_binding) != keystrokes {
                        continue;
                    }

                    if is_no_action(&*disabled_binding.action) {
                        if disabled_binding_matches_context(disabled_binding, binding) {
                            return None;
                        }
                    } else if is_unbind(&*disabled_binding.action)
                        && disabled_binding_matches_context(disabled_binding, binding)
                        && binding_is_unbound(disabled_binding, binding)
                    {
                        return None;
                    }
                }
            }

            Some(binding)
        })
    }

    /// Returns all bindings that might match the input without checking context. The bindings
    /// returned in precedence order (reverse of the order they were added to the keymap).
    pub fn all_bindings_for_input(&self, input: &[Keystroke]) -> Vec<KeyBinding> {
        self.bindings()
            .rev()
            .filter(|binding| {
                binding
                    .match_keystrokes(input)
                    .is_some_and(|pending| !pending)
            })
            .cloned()
            .collect()
    }

    /// Returns a list of bindings that match the given input, and a boolean indicating whether or
    /// not more bindings might match if the input was longer. Bindings are returned in precedence
    /// order (higher precedence first, reverse of the order they were added to the keymap).
    ///
    /// Precedence is defined by the depth in the tree (matches on the Editor take precedence over
    /// matches on the Pane, then the Workspace, etc.). Bindings with no context are treated as the
    /// same as the deepest context.
    ///
    /// In the case of multiple bindings at the same depth, the ones added to the keymap later take
    /// precedence. User bindings are added after built-in bindings so that they take precedence.
    ///
    /// If a binding has been disabled with `"x": null` it will not be returned. Disabled bindings
    /// are evaluated with the same precedence rules so you can disable a rule in a given context
    /// only. A disabled binding only suppresses bindings from sources with equal or weaker
    /// precedence: a base keymap null hides default bindings, but user bindings still apply.
    pub fn bindings_for_input(
        &self,
        input: &[impl AsKeystroke],
        context_stack: &[KeyContext],
    ) -> (SmallVec<[KeyBinding; 1]>, bool) {
        let (bindings, pending) = self.resolve_bindings_for_input(
            input,
            context_stack,
            self.bindings().enumerate().rev(),
        );
        (
            bindings
                .into_iter()
                .map(|(_, binding)| binding.clone())
                .collect(),
            pending,
        )
    }

    /// Same as `bindings_for_input`, but only looks at `keymap_bindings` instead of the whole
    /// keymap, and returns each binding with its keymap index.
    ///
    /// `keymap_bindings` must be in reverse keymap order and include every binding that could
    /// match `input`. For example, leaving out a `null` binding would make the binding it disables
    /// look like it wins. `bindings_for_input` passes the whole keymap, which always meets this but
    /// goes through every binding on every call. Passing a pre-filtered list is cheaper when
    /// resolving many inputs against it.
    fn resolve_bindings_for_input<'a>(
        &self,
        input: &[impl AsKeystroke],
        context_stack: &[KeyContext],
        keymap_bindings: impl IntoIterator<Item = (usize, &'a KeyBinding)>,
    ) -> (SmallVec<[(BindingIndex, &'a KeyBinding); 1]>, bool) {
        let mut matched_bindings = SmallVec::<[(usize, BindingIndex, &KeyBinding); 1]>::new();
        let mut pending_bindings = SmallVec::<[(BindingIndex, &KeyBinding); 1]>::new();

        for (ix, binding) in keymap_bindings {
            let Some(depth) = self.binding_enabled(binding, context_stack) else {
                continue;
            };
            let Some(pending) = binding.match_keystrokes(input) else {
                continue;
            };

            if !pending {
                matched_bindings.push((depth, BindingIndex(ix), binding));
            } else {
                pending_bindings.push((BindingIndex(ix), binding));
            }
        }

        matched_bindings.sort_by(|(depth_a, ix_a, _), (depth_b, ix_b, _)| {
            depth_b.cmp(depth_a).then(ix_b.cmp(ix_a))
        });

        let mut bindings: SmallVec<[_; 1]> = SmallVec::new();
        let mut first_binding_index = None;
        let mut unbound_bindings: Vec<&KeyBinding> = Vec::new();
        // A `NoAction` binding suppresses out-ranked bindings from sources with
        // equal or weaker precedence, while bindings from stronger sources (a
        // smaller meta, e.g. a user binding vs a base keymap null) still apply.
        // Bindings without a meta are treated as user bindings.
        let mut no_action_meta: Option<u32> = None;

        for (_, ix, binding) in matched_bindings {
            let meta = binding.meta.map_or(0, |meta| meta.0);
            if is_no_action(&*binding.action) {
                no_action_meta = Some(no_action_meta.map_or(meta, |existing| existing.min(meta)));
                continue;
            }

            if no_action_meta.is_some_and(|no_action_meta| meta >= no_action_meta) {
                continue;
            }

            if is_unbind(&*binding.action) {
                unbound_bindings.push(binding);
                continue;
            }

            if unbound_bindings
                .iter()
                .any(|disabled_binding| binding_is_unbound(disabled_binding, binding))
            {
                continue;
            }

            bindings.push((ix, binding));
            first_binding_index.get_or_insert(ix);
        }

        let mut pending = HashSet::default();
        for (ix, binding) in pending_bindings.into_iter().rev() {
            if let Some(binding_ix) = first_binding_index
                && binding_ix > ix
            {
                continue;
            }
            if is_no_action(&*binding.action) || is_unbind(&*binding.action) {
                pending.remove(&folded_keystrokes(binding));
                continue;
            }
            pending.insert(folded_keystrokes(binding));
        }

        (bindings, !pending.is_empty())
    }
    /// Check if the given binding is enabled, given a certain key context.
    /// Returns the deepest depth at which the binding matches, or None if it doesn't match.
    fn binding_enabled(&self, binding: &KeyBinding, contexts: &[KeyContext]) -> Option<usize> {
        if let Some(predicate) = &binding.context_predicate {
            predicate.depth_of(contexts)
        } else {
            Some(contexts.len())
        }
    }

    /// Find the bindings that can follow the current input sequence, in precedence order. Only
    /// includes bindings that dispatch would run for their full keystrokes, so bindings disabled
    /// with `null` or `Unbind` are left out.
    pub fn possible_next_bindings_for_input(
        &self,
        input: &[Keystroke],
        context_stack: &[KeyContext],
    ) -> Vec<KeyBinding> {
        // Keeps `null` and `Unbind` bindings, which can disable candidates.
        let bindings_extending_input = self
            .bindings()
            .enumerate()
            .rev()
            .filter(|(_, binding)| {
                binding
                    .match_keystrokes(input)
                    .is_some_and(|pending| pending)
            })
            .collect::<Vec<_>>();

        let mut full_input = SmallVec::<[&Keystroke; 4]>::new();
        let mut candidates = bindings_extending_input
            .iter()
            .filter(|(_, binding)| !is_no_action(&*binding.action) && !is_unbind(&*binding.action))
            .filter_map(|&(ix, binding)| {
                let depth = self.binding_enabled(binding, context_stack)?;
                // Use the typed input rather than the binding's own keystrokes. A typed keystroke
                // can match through its physical key, like `alt-s`, or the character it produced,
                // like `ß`.
                full_input.clear();
                full_input.extend(input.iter());
                full_input.extend(
                    binding.keystrokes[input.len()..]
                        .iter()
                        .map(AsKeystroke::as_keystroke),
                );
                let (dispatched, _) = self.resolve_bindings_for_input(
                    &full_input,
                    context_stack,
                    bindings_extending_input.iter().copied(),
                );
                dispatched
                    .iter()
                    .any(|(dispatched_ix, _)| *dispatched_ix == BindingIndex(ix))
                    .then_some((depth, ix, binding))
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable_by(|(depth_a, ix_a, _), (depth_b, ix_b, _)| {
            depth_b.cmp(depth_a).then(ix_b.cmp(ix_a))
        });

        candidates
            .into_iter()
            .map(|(_, _, binding)| binding.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as gpui;
    use gpui::{NoAction, Unbind};

    actions!(
        test_only,
        [ActionAlpha, ActionBeta, ActionGamma, ActionDelta,]
    );

    #[test]
    fn test_keymap() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionAlpha {}, None),
            KeyBinding::new("ctrl-a", ActionBeta {}, Some("pane")),
            KeyBinding::new("ctrl-a", ActionGamma {}, Some("editor && mode==full")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings.clone());

        // global bindings are enabled in all contexts
        assert_eq!(keymap.binding_enabled(&bindings[0], &[]), Some(0));
        assert_eq!(
            keymap.binding_enabled(&bindings[0], &[KeyContext::parse("terminal").unwrap()]),
            Some(1)
        );

        // contextual bindings are enabled in contexts that match their predicate
        assert_eq!(
            keymap.binding_enabled(&bindings[1], &[KeyContext::parse("barf x=y").unwrap()]),
            None
        );
        assert_eq!(
            keymap.binding_enabled(&bindings[1], &[KeyContext::parse("pane x=y").unwrap()]),
            Some(1)
        );

        assert_eq!(
            keymap.binding_enabled(&bindings[2], &[KeyContext::parse("editor").unwrap()]),
            None
        );
        assert_eq!(
            keymap.binding_enabled(
                &bindings[2],
                &[KeyContext::parse("editor mode=full").unwrap()]
            ),
            Some(1)
        );
    }

    #[test]
    fn test_depth_precedence() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionBeta {}, Some("pane")),
            KeyBinding::new("ctrl-a", ActionGamma {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-a").unwrap()],
            &[
                KeyContext::parse("pane").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ],
        );

        assert!(!pending);
        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionGamma {}));
        assert!(result[1].action.partial_eq(&ActionBeta {}));
    }

    #[test]
    fn test_keymap_disabled() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-b", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-a", NoAction {}, Some("editor && mode==full")),
            KeyBinding::new("ctrl-b", NoAction {}, None),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // binding is only enabled in a specific context
        assert!(
            keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-a").unwrap()],
                    &[KeyContext::parse("barf").unwrap()],
                )
                .0
                .is_empty()
        );
        assert!(
            !keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-a").unwrap()],
                    &[KeyContext::parse("editor").unwrap()],
                )
                .0
                .is_empty()
        );

        // binding is disabled in a more specific context
        assert!(
            keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-a").unwrap()],
                    &[KeyContext::parse("editor mode=full").unwrap()],
                )
                .0
                .is_empty()
        );

        // binding is globally disabled
        assert!(
            keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-b").unwrap()],
                    &[KeyContext::parse("barf").unwrap()],
                )
                .0
                .is_empty()
        );
    }

    #[test]
    /// Tests for https://github.com/zed-industries/zed/issues/30259
    fn test_multiple_keystroke_binding_disabled() {
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let space = || Keystroke::parse("space").unwrap();
        let w = || Keystroke::parse("w").unwrap();

        let space_w = [space(), w()];
        let space_w_w = [space(), w(), w()];

        let workspace_context = || [KeyContext::parse("workspace").unwrap()];

        let editor_workspace_context = || {
            [
                KeyContext::parse("workspace").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ]
        };

        // Ensure `space` results in pending input on the workspace, but not editor
        let space_workspace = keymap.bindings_for_input(&[space()], &workspace_context());
        assert!(space_workspace.0.is_empty());
        assert!(space_workspace.1);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(!space_editor.1);

        // Ensure `space w` results in pending input on the workspace, but not editor
        let space_w_workspace = keymap.bindings_for_input(&space_w, &workspace_context());
        assert!(space_w_workspace.0.is_empty());
        assert!(space_w_workspace.1);

        let space_w_editor = keymap.bindings_for_input(&space_w, &editor_workspace_context());
        assert!(space_w_editor.0.is_empty());
        assert!(!space_w_editor.1);

        // Ensure `space w w` results in the binding in the workspace, but not in the editor
        let space_w_w_workspace = keymap.bindings_for_input(&space_w_w, &workspace_context());
        assert!(!space_w_w_workspace.0.is_empty());
        assert!(!space_w_w_workspace.1);

        let space_w_w_editor = keymap.bindings_for_input(&space_w_w, &editor_workspace_context());
        assert!(space_w_w_editor.0.is_empty());
        assert!(!space_w_w_editor.1);

        // Now test what happens if we have another binding defined AFTER the NoAction
        // that should result in pending
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
            KeyBinding::new("space w x", ActionAlpha {}, Some("editor")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(space_editor.1);

        // Now test what happens if we have another binding defined BEFORE the NoAction
        // that should result in pending
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w x", ActionAlpha {}, Some("editor")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(space_editor.1);

        // Now test what happens if we have another binding defined at a higher context
        // that should result in pending
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w x", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(space_editor.1);
    }

    #[test]
    fn test_override_multikey() {
        let bindings = [
            KeyBinding::new("ctrl-w left", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-w", NoAction {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-w").unwrap()],
            &[KeyContext::parse("editor").unwrap()],
        );
        assert!(result.is_empty());
        assert!(pending);

        let bindings = [
            KeyBinding::new("ctrl-w left", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-w", ActionBeta {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-w").unwrap()],
            &[KeyContext::parse("editor").unwrap()],
        );
        assert_eq!(result.len(), 1);
        assert!(!pending);
    }

    #[test]
    fn test_simple_disable() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x").unwrap()],
            &[KeyContext::parse("editor").unwrap()],
        );
        assert!(result.is_empty());
        assert!(!pending);
    }

    #[test]
    fn test_disable_weaker_sources_only() {
        const USER: KeyBindingMetaIndex = KeyBindingMetaIndex(0);
        const VIM: KeyBindingMetaIndex = KeyBindingMetaIndex(1);
        const BASE: KeyBindingMetaIndex = KeyBindingMetaIndex(2);
        const DEFAULT: KeyBindingMetaIndex = KeyBindingMetaIndex(3);

        let editor_context = || [KeyContext::parse("editor").unwrap()];
        let ctrl_x = || [Keystroke::parse("ctrl-x").unwrap()];

        // A base keymap null disables a default binding in the same context.
        let mut keymap = Keymap::default();
        keymap.add_bindings([
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")).with_meta(DEFAULT),
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(BASE),
        ]);
        let (result, _) = keymap.bindings_for_input(&ctrl_x(), &editor_context());
        assert!(result.is_empty());

        // A user binding is not affected by base keymap or default nulls.
        let mut keymap = Keymap::default();
        keymap.add_bindings([
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(DEFAULT),
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(BASE),
            KeyBinding::new("ctrl-x", ActionBeta {}, None).with_meta(USER),
        ]);
        let (result, _) = keymap.bindings_for_input(&ctrl_x(), &editor_context());
        assert_eq!(result.len(), 1);
        assert!(result[0].action.partial_eq(&ActionBeta {}));

        // A user binding at a shallower context is not disabled by a deeper
        // base keymap null.
        let mut keymap = Keymap::default();
        keymap.add_bindings([
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(BASE),
            KeyBinding::new("ctrl-x", ActionBeta {}, Some("workspace")).with_meta(USER),
        ]);
        let (result, _) = keymap.bindings_for_input(
            &ctrl_x(),
            &[
                KeyContext::parse("workspace").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ],
        );
        assert_eq!(result.len(), 1);
        assert!(result[0].action.partial_eq(&ActionBeta {}));

        // A vim binding survives a base keymap null, and a user null disables
        // everything.
        let mut keymap = Keymap::default();
        keymap.add_bindings([
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")).with_meta(DEFAULT),
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(BASE),
            KeyBinding::new("ctrl-x", ActionGamma {}, Some("editor")).with_meta(VIM),
        ]);
        let (result, _) = keymap.bindings_for_input(&ctrl_x(), &editor_context());
        assert_eq!(result.len(), 1);
        assert!(result[0].action.partial_eq(&ActionGamma {}));

        let mut keymap = Keymap::default();
        keymap.add_bindings([
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")).with_meta(DEFAULT),
            KeyBinding::new("ctrl-x", ActionGamma {}, Some("editor")).with_meta(VIM),
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(USER),
        ]);
        let (result, _) = keymap.bindings_for_input(&ctrl_x(), &editor_context());
        assert!(result.is_empty());
    }

    #[test]
    fn test_fail_to_disable() {
        // disabled at the wrong level
        let bindings = [
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-x", NoAction {}, Some("workspace")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x").unwrap()],
            &[
                KeyContext::parse("workspace").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ],
        );
        assert_eq!(result.len(), 1);
        assert!(!pending);
    }

    #[test]
    fn test_disable_deeper() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x").unwrap()],
            &[
                KeyContext::parse("workspace").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ],
        );
        assert_eq!(result.len(), 0);
        assert!(!pending);
    }

    #[test]
    fn test_pending_match_enabled() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionBeta, Some("vim_mode == normal")),
            KeyBinding::new("ctrl-x 0", ActionAlpha, Some("Workspace")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(matched.1);
    }

    #[test]
    fn test_pending_match_enabled_extended() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionBeta, Some("vim_mode == normal")),
            KeyBinding::new("ctrl-x 0", NoAction, Some("Workspace")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(!matched.1);
        let bindings = [
            KeyBinding::new("ctrl-x", ActionBeta, Some("Workspace")),
            KeyBinding::new("ctrl-x 0", NoAction, Some("vim_mode == normal")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(!matched.1);
    }

    #[test]
    fn test_overriding_prefix() {
        let bindings = [
            KeyBinding::new("ctrl-x 0", ActionAlpha, Some("Workspace")),
            KeyBinding::new("ctrl-x", ActionBeta, Some("vim_mode == normal")),
        ];
        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(!matched.1);
    }

    #[test]
    fn test_context_precedence_with_same_source() {
        // Test case: User has both Workspace and Editor bindings for the same key
        // Editor binding should take precedence over Workspace binding
        let bindings = [
            KeyBinding::new("cmd-r", ActionAlpha {}, Some("Workspace")),
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        // Test with context stack: [Workspace, Editor] (Editor is deeper)
        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        // Both bindings should be returned, but Editor binding should be first (highest precedence)
        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionBeta {})); // Editor binding first
        assert!(result[1].action.partial_eq(&ActionAlpha {})); // Workspace binding second
    }

    #[test]
    fn test_bindings_for_action() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionAlpha {}, Some("pane")),
            KeyBinding::new("ctrl-b", ActionBeta {}, Some("editor && mode == full")),
            KeyBinding::new("ctrl-c", ActionGamma {}, Some("workspace")),
            KeyBinding::new("ctrl-a", NoAction {}, Some("pane && active")),
            KeyBinding::new("ctrl-b", NoAction {}, Some("editor")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        assert_bindings(&keymap, &ActionAlpha {}, &["ctrl-a"]);
        assert_bindings(&keymap, &ActionBeta {}, &[]);
        assert_bindings(&keymap, &ActionGamma {}, &["ctrl-c"]);

        #[track_caller]
        fn assert_bindings(keymap: &Keymap, action: &dyn Action, expected: &[&str]) {
            let actual = keymap
                .bindings_for_action(action)
                .map(|binding| {
                    binding.keystrokes[0]
                        .inner()
                        .unparse(crate::OperatingSystem::Mac)
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{:?}", action);
        }
    }

    #[test]
    fn test_targeted_unbind_ignores_target_context() {
        let bindings = [
            KeyBinding::new("tab", ActionAlpha {}, Some("Editor")),
            KeyBinding::new("tab", ActionBeta {}, Some("Editor && showing_completions")),
            KeyBinding::new(
                "tab",
                Unbind("test_only::ActionAlpha".into()),
                Some("Editor && edit_prediction"),
            ),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("tab").unwrap()],
            &[KeyContext::parse("Editor showing_completions edit_prediction").unwrap()],
        );

        assert!(!pending);
        assert_eq!(result.len(), 1);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
    }

    #[test]
    fn test_bindings_for_action_keeps_binding_for_narrower_targeted_unbind() {
        let bindings = [
            KeyBinding::new("tab", ActionAlpha {}, Some("Editor")),
            KeyBinding::new(
                "tab",
                Unbind("test_only::ActionAlpha".into()),
                Some("Editor && edit_prediction"),
            ),
            KeyBinding::new("tab", ActionBeta {}, Some("Editor && showing_completions")),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        assert_bindings(&keymap, &ActionAlpha {}, &["tab"]);
        assert_bindings(&keymap, &ActionBeta {}, &["tab"]);

        #[track_caller]
        fn assert_bindings(keymap: &Keymap, action: &dyn Action, expected: &[&str]) {
            let actual = keymap
                .bindings_for_action(action)
                .map(|binding| {
                    binding.keystrokes[0]
                        .inner()
                        .unparse(crate::OperatingSystem::Mac)
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{:?}", action);
        }
    }

    #[test]
    fn test_bindings_for_action_removes_binding_for_broader_targeted_unbind() {
        let bindings = [
            KeyBinding::new("tab", ActionAlpha {}, Some("Editor && edit_prediction")),
            KeyBinding::new(
                "tab",
                Unbind("test_only::ActionAlpha".into()),
                Some("Editor"),
            ),
        ];

        let mut keymap = Keymap::default();
        keymap.add_bindings(bindings);

        assert!(keymap.bindings_for_action(&ActionAlpha {}).next().is_none());
    }

    #[test]
    fn test_source_precedence_sorting() {
        // KeybindSource precedence: User (0) > Vim (1) > Base (2) > Default (3)
        // Test that user keymaps take precedence over default keymaps at the same context depth
        let mut keymap = Keymap::default();

        // Add a default keymap binding first
        let mut default_binding = KeyBinding::new("cmd-r", ActionAlpha {}, Some("Editor"));
        default_binding.set_meta(KeyBindingMetaIndex(3)); // Default source
        keymap.add_bindings([default_binding]);

        // Add a user keymap binding
        let mut user_binding = KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor"));
        user_binding.set_meta(KeyBindingMetaIndex(0)); // User source
        keymap.add_bindings([user_binding]);

        // Test with Editor context stack
        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[KeyContext::parse("Editor").unwrap()],
        );

        // User binding should take precedence over default binding
        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
        assert!(result[1].action.partial_eq(&ActionAlpha {}));
    }

    #[test]
    fn test_possible_next_bindings_exclude_null_bindings() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-k a", ActionAlpha {}, Some("Editor")),
            KeyBinding::new("ctrl-k b", ActionBeta {}, Some("Editor")),
            KeyBinding::new("ctrl-k a", NoAction {}, Some("Editor")),
        ]);

        let bindings = keymap.possible_next_bindings_for_input(
            &[Keystroke::parse("ctrl-k").unwrap()],
            &[KeyContext::parse("Editor").unwrap()],
        );
        assert_eq!(
            describe_bindings(&bindings),
            vec![("ctrl-k b".to_string(), "test_only::ActionBeta")]
        );
    }

    #[test]
    fn test_possible_next_bindings_exclude_unbound_bindings() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-k a", ActionAlpha {}, Some("Editor")),
            KeyBinding::new("ctrl-k b", ActionBeta {}, Some("Editor")),
            KeyBinding::new(
                "ctrl-k a",
                Unbind("test_only::ActionAlpha".into()),
                Some("Editor"),
            ),
        ]);

        let bindings = keymap.possible_next_bindings_for_input(
            &[Keystroke::parse("ctrl-k").unwrap()],
            &[KeyContext::parse("Editor").unwrap()],
        );
        assert_eq!(
            describe_bindings(&bindings),
            vec![("ctrl-k b".to_string(), "test_only::ActionBeta")]
        );
    }

    #[test]
    fn test_possible_next_bindings_keep_user_binding_over_base_keymap_null() {
        const USER: KeyBindingMetaIndex = KeyBindingMetaIndex(0);
        const BASE: KeyBindingMetaIndex = KeyBindingMetaIndex(2);
        const DEFAULT: KeyBindingMetaIndex = KeyBindingMetaIndex(3);

        let mut keymap = Keymap::default();
        keymap.add_bindings([
            KeyBinding::new("ctrl-k a", ActionAlpha {}, Some("Editor")).with_meta(DEFAULT),
            KeyBinding::new("ctrl-k a", NoAction {}, Some("Editor")).with_meta(BASE),
            KeyBinding::new("ctrl-k a", ActionBeta {}, Some("Editor")).with_meta(USER),
        ]);

        let bindings = keymap.possible_next_bindings_for_input(
            &[Keystroke::parse("ctrl-k").unwrap()],
            &[KeyContext::parse("Editor").unwrap()],
        );
        assert_eq!(
            describe_bindings(&bindings),
            vec![("ctrl-k a".to_string(), "test_only::ActionBeta")]
        );
    }

    #[test]
    fn test_possible_next_bindings_apply_disabling_by_context() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-k a", ActionAlpha {}, Some("Workspace")),
            KeyBinding::new("ctrl-k a", ActionBeta {}, Some("Editor")),
            KeyBinding::new("ctrl-k b", ActionGamma {}, Some("Editor")),
            // Only removes the editor binding, so the workspace binding still dispatches.
            KeyBinding::new(
                "ctrl-k a",
                Unbind("test_only::ActionBeta".into()),
                Some("Editor"),
            ),
            // Doesn't apply because `Terminal` isn't in the context stack.
            KeyBinding::new("ctrl-k b", NoAction {}, Some("Terminal")),
        ]);

        let bindings = keymap.possible_next_bindings_for_input(
            &[Keystroke::parse("ctrl-k").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );
        assert_eq!(
            describe_bindings(&bindings),
            vec![
                ("ctrl-k b".to_string(), "test_only::ActionGamma"),
                ("ctrl-k a".to_string(), "test_only::ActionAlpha"),
            ]
        );
    }

    #[test]
    fn test_possible_next_bindings_exclude_longer_chord_disabled_in_deeper_context() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("space w w", ActionAlpha {}, Some("Workspace")),
            KeyBinding::new("space w x", ActionBeta {}, Some("Workspace")),
            KeyBinding::new("space w w", NoAction {}, Some("AgentPanel")),
        ]);
        let context_stack = [
            KeyContext::parse("Workspace").unwrap(),
            KeyContext::parse("AgentPanel").unwrap(),
        ];

        for input in ["space", "space w"] {
            let input = input
                .split(' ')
                .map(|keystroke| Keystroke::parse(keystroke).unwrap())
                .collect::<Vec<_>>();
            let bindings = keymap.possible_next_bindings_for_input(&input, &context_stack);
            assert_eq!(
                describe_bindings(&bindings),
                vec![("space w x".to_string(), "test_only::ActionBeta")]
            );
        }
    }

    #[test]
    fn test_possible_next_bindings_exclude_disabled_bindings_with_typed_input() {
        // Option-S reports both its physical key and the character it produced, so typed input
        // matches bindings written with either.
        let typed_input = [Keystroke::parse("alt-s->ß").unwrap()];
        let keymap = Keymap::new(vec![
            KeyBinding::new("ß a", ActionAlpha {}, None),
            KeyBinding::new("alt-s a", NoAction {}, None),
        ]);
        assert!(
            keymap
                .possible_next_bindings_for_input(&typed_input, &[])
                .is_empty()
        );

        // Cross-check against dispatch of the same typed input.
        let typed_then_a = [
            Keystroke::parse("alt-s->ß").unwrap(),
            Keystroke::parse("a").unwrap(),
        ];
        let (dispatched, _) = keymap.bindings_for_input(&typed_then_a, &[]);
        assert!(dispatched.is_empty());
    }

    fn typed(source: &'static str) -> Keystroke {
        Keystroke::parse(source).unwrap()
    }

    /// The bindings that run for the input, as `describe_bindings` shows them.
    #[track_caller]
    fn dispatched(
        keymap: &Keymap,
        input: &[&'static str],
        contexts: &[&str],
    ) -> Vec<(String, &'static str)> {
        let input = input.iter().map(|source| typed(source)).collect::<Vec<_>>();
        let contexts = contexts
            .iter()
            .map(|context| KeyContext::parse(context).unwrap())
            .collect::<Vec<_>>();
        let (bindings, pending) = keymap.bindings_for_input(&input, &contexts);
        assert!(!pending, "{input:?} should not be pending");
        describe_bindings(&bindings)
    }

    fn alpha(keystrokes: &str) -> (String, &'static str) {
        (keystrokes.to_string(), "test_only::ActionAlpha")
    }

    fn beta(keystrokes: &str) -> (String, &'static str) {
        (keystrokes.to_string(), "test_only::ActionBeta")
    }

    #[test]
    fn test_shifted_symbol_bindings_match_what_the_platforms_send() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-shift-\\", ActionAlpha {}, None),
            KeyBinding::new("ctrl-@", ActionBeta {}, None),
            KeyBinding::new("ctrl-shift-p", ActionGamma {}, None),
            KeyBinding::new("cmd-shift-\\", ActionDelta {}, None),
        ]);
        let gamma = ("ctrl-shift-p".to_string(), "test_only::ActionGamma");
        let delta = (
            typed("cmd-shift-\\").unparse(crate::OperatingSystem::Mac),
            "test_only::ActionDelta",
        );

        // Ctrl+Shift+\ on Windows and Linux, and Ctrl+Shift+2, send the symbol without Shift.
        assert_eq!(
            dispatched(&keymap, &["ctrl-|"], &[]),
            vec![alpha("ctrl-shift-\\")]
        );
        assert_eq!(dispatched(&keymap, &["ctrl-@"], &[]), vec![beta("ctrl-@")]);
        // Cmd+Shift+\ on macOS.
        assert_eq!(dispatched(&keymap, &["cmd-|"], &[]), vec![delta]);
        // The web sends Shift along with the symbol, and a test types the chord as written.
        assert_eq!(
            dispatched(&keymap, &["ctrl-shift-|"], &[]),
            vec![alpha("ctrl-shift-\\")]
        );
        assert_eq!(
            dispatched(&keymap, &["ctrl-shift-\\"], &[]),
            vec![alpha("ctrl-shift-\\")]
        );
        assert_eq!(
            dispatched(&keymap, &["ctrl-shift-2"], &[]),
            vec![beta("ctrl-@")]
        );
        // Letters keep their Shift.
        assert_eq!(dispatched(&keymap, &["ctrl-P"], &[]), vec![gamma]);
        assert!(dispatched(&keymap, &["ctrl-p"], &[]).is_empty());
        // Other chords match nothing.
        for input in [
            "ctrl-\\",
            "ctrl-2",
            "alt-|",
            "ctrl-alt-|",
            "|",
            "shift-|",
            "ctrl-/",
        ] {
            assert!(dispatched(&keymap, &[input], &[]).is_empty(), "{input}");
        }
    }

    #[test]
    fn test_one_chord_spelled_two_ways_runs_the_later_binding_first() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-|", ActionAlpha {}, None),
            KeyBinding::new("ctrl-shift-\\", ActionBeta {}, None),
        ]);
        for input in ["ctrl-|", "ctrl-shift-\\"] {
            assert_eq!(
                dispatched(&keymap, &[input], &[]),
                vec![beta("ctrl-shift-\\"), alpha("ctrl-|")]
            );
        }
        let all = keymap.all_bindings_for_input(&[typed("ctrl-|")]);
        assert_eq!(
            describe_bindings(&all),
            vec![beta("ctrl-shift-\\"), alpha("ctrl-|")]
        );

        let keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-shift-\\", ActionBeta {}, None),
            KeyBinding::new("ctrl-|", ActionAlpha {}, None),
        ]);
        assert_eq!(
            dispatched(&keymap, &["ctrl-|"], &[]),
            vec![alpha("ctrl-|"), beta("ctrl-shift-\\")]
        );
    }

    #[test]
    fn test_shifted_symbol_bindings_follow_their_context() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-|", ActionAlpha {}, Some("workspace")),
            KeyBinding::new(
                "ctrl-shift-\\",
                ActionBeta {},
                Some("editor && mode == full"),
            ),
        ]);
        let both = vec![beta("ctrl-shift-\\"), alpha("ctrl-|")];

        assert!(dispatched(&keymap, &["ctrl-|"], &["pane"]).is_empty());
        assert_eq!(
            dispatched(&keymap, &["ctrl-|"], &["workspace"]),
            vec![alpha("ctrl-|")]
        );
        assert_eq!(
            dispatched(&keymap, &["ctrl-|"], &["workspace", "editor"]),
            vec![alpha("ctrl-|")]
        );
        assert_eq!(
            dispatched(&keymap, &["ctrl-|"], &["workspace", "editor mode=full"]),
            both
        );
        assert_eq!(
            dispatched(
                &keymap,
                &["ctrl-shift-|"],
                &["workspace", "editor mode=full"]
            ),
            both
        );
    }

    #[test]
    fn test_a_shifted_symbol_can_be_disabled_by_either_spelling() {
        for (bound, disabled) in [("ctrl-shift-\\", "ctrl-|"), ("ctrl-|", "ctrl-shift-\\")] {
            let keymap = Keymap::new(vec![
                KeyBinding::new(bound, ActionAlpha {}, None),
                KeyBinding::new(disabled, NoAction {}, None),
            ]);
            assert!(dispatched(&keymap, &["ctrl-|"], &[]).is_empty());
        }
    }

    /// `null` and `Unbind` of `ActionAlpha`, which disable the same bindings.
    fn disabling(keystrokes: &'static str) -> [KeyBinding; 2] {
        [
            KeyBinding::new(keystrokes, NoAction {}, None),
            KeyBinding::new(keystrokes, Unbind("test_only::ActionAlpha".into()), None),
        ]
    }

    #[test]
    fn test_unbind_removes_a_binding_in_either_spelling() {
        for (bound, unbound) in [
            ("ctrl-shift-\\", "ctrl-shift-\\"),
            ("ctrl-|", "ctrl-|"),
            ("ctrl-shift-\\", "ctrl-|"),
            ("ctrl-|", "ctrl-shift-\\"),
        ] {
            let keymap = Keymap::new(vec![
                KeyBinding::new(bound, ActionAlpha {}, None),
                KeyBinding::new(unbound, Unbind("test_only::ActionAlpha".into()), None),
            ]);
            assert!(
                dispatched(&keymap, &["ctrl-|"], &[]).is_empty(),
                "{bound} unbound by {unbound}"
            );
        }
    }

    #[test]
    fn test_a_pending_sequence_is_disabled_by_either_spelling() {
        for (bound, disabled) in [
            ("ctrl-shift-\\ a", "ctrl-| a"),
            ("ctrl-| a", "ctrl-shift-\\ a"),
        ] {
            for disabling in disabling(disabled) {
                let keymap = Keymap::new(vec![
                    KeyBinding::new(bound, ActionAlpha {}, None),
                    disabling,
                ]);
                let (bindings, pending) = keymap.bindings_for_input(&[typed("ctrl-|")], &[]);
                assert!(bindings.is_empty());
                assert!(!pending, "{bound} disabled by {disabled}");
            }
        }
    }

    #[test]
    fn test_bindings_for_action_skips_a_binding_disabled_in_either_spelling() {
        for (bound, disabled) in [
            ("ctrl-shift-\\", "ctrl-|"),
            ("ctrl-|", "ctrl-shift-\\"),
            ("ctrl-k ctrl-shift-\\", "ctrl-k ctrl-|"),
        ] {
            for disabling in disabling(disabled) {
                let keymap = Keymap::new(vec![
                    KeyBinding::new(bound, ActionAlpha {}, None),
                    disabling,
                ]);
                assert!(
                    keymap.bindings_for_action(&ActionAlpha {}).next().is_none(),
                    "{bound} disabled by {disabled}"
                );
            }
        }
    }

    #[test]
    fn test_shifted_symbols_in_a_sequence_of_keystrokes() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-k ctrl-shift-\\", ActionAlpha {}, None),
            KeyBinding::new("ctrl-k x", ActionBeta {}, None),
            KeyBinding::new("ctrl-shift-1 a", ActionGamma {}, None),
        ]);

        let (bindings, pending) = keymap.bindings_for_input(&[typed("ctrl-k")], &[]);
        assert!(bindings.is_empty());
        assert!(pending);
        assert_eq!(
            dispatched(&keymap, &["ctrl-k", "ctrl-|"], &[]),
            vec![alpha("ctrl-k ctrl-shift-\\")]
        );
        assert!(dispatched(&keymap, &["ctrl-k", "ctrl-\\"], &[]).is_empty());

        // The first keystroke of a sequence is the symbol a platform sends.
        let (bindings, pending) = keymap.bindings_for_input(&[typed("ctrl-!")], &[]);
        assert!(bindings.is_empty());
        assert!(pending);
        assert_eq!(
            dispatched(&keymap, &["ctrl-!", "a"], &[]),
            vec![("ctrl-shift-1 a".to_string(), "test_only::ActionGamma")]
        );
        assert!(dispatched(&keymap, &["ctrl-!", "b"], &[]).is_empty());

        let next = keymap.possible_next_bindings_for_input(&[typed("ctrl-k")], &[]);
        assert_eq!(
            describe_bindings(&next),
            vec![beta("ctrl-k x"), alpha("ctrl-k ctrl-shift-\\")]
        );
        let next = keymap.possible_next_bindings_for_input(&[typed("ctrl-!")], &[]);
        assert_eq!(
            describe_bindings(&next),
            vec![("ctrl-shift-1 a".to_string(), "test_only::ActionGamma")]
        );

        let binding = keymap.bindings().next().unwrap();
        assert_eq!(binding.match_keystrokes(&[typed("ctrl-k")]), Some(true));
        assert_eq!(
            binding.match_keystrokes(&[typed("ctrl-k"), typed("ctrl-|")]),
            Some(false)
        );
        assert_eq!(
            binding.match_keystrokes(&[typed("ctrl-k"), typed("ctrl-/")]),
            None
        );
    }

    #[test]
    fn test_a_shifted_symbol_binding_keeps_its_spelling() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-shift-\\", ActionAlpha {}, None),
            KeyBinding::new("ctrl-@", ActionBeta {}, None),
        ]);
        for (action, expected) in [
            (&ActionAlpha {} as &dyn Action, "ctrl-shift-\\"),
            (&ActionBeta {}, "ctrl-@"),
        ] {
            let spelled = keymap
                .bindings_for_action(action)
                .map(|binding| {
                    binding.keystrokes[0]
                        .inner()
                        .unparse(crate::OperatingSystem::Mac)
                })
                .collect::<Vec<_>>();
            assert_eq!(spelled, [expected]);
        }
    }

    fn describe_bindings(bindings: &[KeyBinding]) -> Vec<(String, &'static str)> {
        bindings
            .iter()
            .map(|binding| {
                (
                    binding
                        .keystrokes()
                        .iter()
                        .map(|keystroke| keystroke.inner().unparse(crate::OperatingSystem::Mac))
                        .collect::<Vec<_>>()
                        .join(" "),
                    binding.action().name(),
                )
            })
            .collect()
    }
}
