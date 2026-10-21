//! The command tables (upstream `packages/latex/src/symbols.ts`).

use crate::render::latex::types::SymbolRole;

pub(crate) struct SymbolDefinition {
    pub value: &'static str,
    pub role: SymbolRole,
}

/// `symbolTable` — ordinary + binary + relation + arrows + punctuation
/// families, each with its spacing role.
// The tables mirror upstream's per-name entries one-to-one; alias arms
// (vee/lor, ldots/dots, …) intentionally keep the upstream spelling
// order, so identical bodies stay as separate documented rows.
#[allow(clippy::too_many_lines, clippy::match_same_arms)]
pub(crate) fn lookup_symbol(command: &str) -> Option<SymbolDefinition> {
    let (value, role) = match command {
        // ordinary (Greek + letters + misc)
        "alpha" => ("α", SymbolRole::Ordinary),
        "beta" => ("β", SymbolRole::Ordinary),
        "gamma" => ("γ", SymbolRole::Ordinary),
        "delta" => ("δ", SymbolRole::Ordinary),
        "epsilon" => ("ε", SymbolRole::Ordinary),
        "varepsilon" => ("ϵ", SymbolRole::Ordinary),
        "zeta" => ("ζ", SymbolRole::Ordinary),
        "eta" => ("η", SymbolRole::Ordinary),
        "theta" => ("θ", SymbolRole::Ordinary),
        "vartheta" => ("ϑ", SymbolRole::Ordinary),
        "iota" => ("ι", SymbolRole::Ordinary),
        "kappa" => ("κ", SymbolRole::Ordinary),
        "lambda" => ("λ", SymbolRole::Ordinary),
        "mu" => ("μ", SymbolRole::Ordinary),
        "nu" => ("ν", SymbolRole::Ordinary),
        "xi" => ("ξ", SymbolRole::Ordinary),
        "omicron" => ("ο", SymbolRole::Ordinary),
        "pi" => ("π", SymbolRole::Ordinary),
        "varpi" => ("ϖ", SymbolRole::Ordinary),
        "rho" => ("ρ", SymbolRole::Ordinary),
        "varrho" => ("ϱ", SymbolRole::Ordinary),
        "sigma" => ("σ", SymbolRole::Ordinary),
        "varsigma" => ("ς", SymbolRole::Ordinary),
        "tau" => ("τ", SymbolRole::Ordinary),
        "upsilon" => ("υ", SymbolRole::Ordinary),
        "phi" => ("ϕ", SymbolRole::Ordinary),
        "varphi" => ("φ", SymbolRole::Ordinary),
        "chi" => ("χ", SymbolRole::Ordinary),
        "psi" => ("ψ", SymbolRole::Ordinary),
        "omega" => ("ω", SymbolRole::Ordinary),
        "Gamma" => ("Γ", SymbolRole::Ordinary),
        "Delta" => ("Δ", SymbolRole::Ordinary),
        "Theta" => ("Θ", SymbolRole::Ordinary),
        "Lambda" => ("Λ", SymbolRole::Ordinary),
        "Xi" => ("Ξ", SymbolRole::Ordinary),
        "Pi" => ("Π", SymbolRole::Ordinary),
        "Sigma" => ("Σ", SymbolRole::Ordinary),
        "Upsilon" => ("Υ", SymbolRole::Ordinary),
        "Phi" => ("Φ", SymbolRole::Ordinary),
        "Psi" => ("Ψ", SymbolRole::Ordinary),
        "Omega" => ("Ω", SymbolRole::Ordinary),
        "infty" => ("∞", SymbolRole::Ordinary),
        "ell" => ("ℓ", SymbolRole::Ordinary),
        "hbar" => ("ℏ", SymbolRole::Ordinary),
        "imath" => ("ı", SymbolRole::Ordinary),
        "jmath" => ("ȷ", SymbolRole::Ordinary),
        "Re" => ("ℜ", SymbolRole::Ordinary),
        "Im" => ("ℑ", SymbolRole::Ordinary),
        "aleph" => ("ℵ", SymbolRole::Ordinary),
        "beth" => ("ℶ", SymbolRole::Ordinary),
        "gimel" => ("ℷ", SymbolRole::Ordinary),
        "daleth" => ("ℸ", SymbolRole::Ordinary),
        "partial" => ("∂", SymbolRole::Ordinary),
        "nabla" => ("∇", SymbolRole::Ordinary),
        "angle" => ("∠", SymbolRole::Ordinary),
        "measuredangle" => ("∡", SymbolRole::Ordinary),
        "triangle" => ("△", SymbolRole::Ordinary),
        "square" => ("□", SymbolRole::Ordinary),
        "lozenge" => ("◊", SymbolRole::Ordinary),
        "top" => ("⊤", SymbolRole::Ordinary),
        "bot" => ("⊥", SymbolRole::Ordinary),
        "emptyset" => ("∅", SymbolRole::Ordinary),
        "varnothing" => ("∅", SymbolRole::Ordinary),
        "forall" => ("∀", SymbolRole::Ordinary),
        "exists" => ("∃", SymbolRole::Ordinary),
        "nexists" => ("∄", SymbolRole::Ordinary),
        "neg" => ("¬", SymbolRole::Ordinary),
        "lnot" => ("¬", SymbolRole::Ordinary),
        "prime" => ("′", SymbolRole::Ordinary),
        "backprime" => ("‵", SymbolRole::Ordinary),
        "clubsuit" => ("♣", SymbolRole::Ordinary),
        "diamondsuit" => ("♢", SymbolRole::Ordinary),
        "heartsuit" => ("♡", SymbolRole::Ordinary),
        "spadesuit" => ("♠", SymbolRole::Ordinary),
        "checkmark" => ("✓", SymbolRole::Ordinary),
        // binary
        "pm" => ("±", SymbolRole::Binary),
        "mp" => ("∓", SymbolRole::Binary),
        "times" => ("×", SymbolRole::Binary),
        "div" => ("÷", SymbolRole::Binary),
        "cdot" => ("·", SymbolRole::Binary),
        "ast" => ("∗", SymbolRole::Binary),
        "star" => ("⋆", SymbolRole::Binary),
        "circ" => ("∘", SymbolRole::Binary),
        "bullet" => ("∙", SymbolRole::Binary),
        "oplus" => ("⊕", SymbolRole::Binary),
        "ominus" => ("⊖", SymbolRole::Binary),
        "otimes" => ("⊗", SymbolRole::Binary),
        "oslash" => ("⊘", SymbolRole::Binary),
        "odot" => ("⊙", SymbolRole::Binary),
        "cap" => ("∩", SymbolRole::Binary),
        "cup" => ("∪", SymbolRole::Binary),
        "uplus" => ("⊎", SymbolRole::Binary),
        "sqcap" => ("⊓", SymbolRole::Binary),
        "sqcup" => ("⊔", SymbolRole::Binary),
        "vee" => ("∨", SymbolRole::Binary),
        "lor" => ("∨", SymbolRole::Binary),
        "wedge" => ("∧", SymbolRole::Binary),
        "land" => ("∧", SymbolRole::Binary),
        "setminus" => ("∖", SymbolRole::Binary),
        "wr" => ("≀", SymbolRole::Binary),
        "diamond" => ("⋄", SymbolRole::Binary),
        "bigtriangleup" => ("△", SymbolRole::Binary),
        "bigtriangledown" => ("▽", SymbolRole::Binary),
        "triangleleft" => ("◁", SymbolRole::Binary),
        "triangleright" => ("▷", SymbolRole::Binary),
        // relation
        "equals" => ("=", SymbolRole::Relation),
        "neq" => ("≠", SymbolRole::Relation),
        "ne" => ("≠", SymbolRole::Relation),
        "equiv" => ("≡", SymbolRole::Relation),
        "approx" => ("≈", SymbolRole::Relation),
        "sim" => ("∼", SymbolRole::Relation),
        "simeq" => ("≃", SymbolRole::Relation),
        "cong" => ("≅", SymbolRole::Relation),
        "asymp" => ("≍", SymbolRole::Relation),
        "propto" => ("∝", SymbolRole::Relation),
        "lt" => ("<", SymbolRole::Relation),
        "gt" => (">", SymbolRole::Relation),
        "le" => ("≤", SymbolRole::Relation),
        "leq" => ("≤", SymbolRole::Relation),
        "ge" => ("≥", SymbolRole::Relation),
        "geq" => ("≥", SymbolRole::Relation),
        "ll" => ("≪", SymbolRole::Relation),
        "gg" => ("≫", SymbolRole::Relation),
        "prec" => ("≺", SymbolRole::Relation),
        "succ" => ("≻", SymbolRole::Relation),
        "preceq" => ("⪯", SymbolRole::Relation),
        "succeq" => ("⪰", SymbolRole::Relation),
        "subset" => ("⊂", SymbolRole::Relation),
        "supset" => ("⊃", SymbolRole::Relation),
        "subseteq" => ("⊆", SymbolRole::Relation),
        "supseteq" => ("⊇", SymbolRole::Relation),
        "sqsubset" => ("⊏", SymbolRole::Relation),
        "sqsupset" => ("⊐", SymbolRole::Relation),
        "sqsubseteq" => ("⊑", SymbolRole::Relation),
        "sqsupseteq" => ("⊒", SymbolRole::Relation),
        "in" => ("∈", SymbolRole::Relation),
        "ni" => ("∋", SymbolRole::Relation),
        "notin" => ("∉", SymbolRole::Relation),
        "owns" => ("∋", SymbolRole::Relation),
        "vdash" => ("⊢", SymbolRole::Relation),
        "dashv" => ("⊣", SymbolRole::Relation),
        "models" => ("⊨", SymbolRole::Relation),
        "mid" => ("∣", SymbolRole::Relation),
        "parallel" => ("∥", SymbolRole::Relation),
        "perp" => ("⊥", SymbolRole::Relation),
        "smile" => ("⌣", SymbolRole::Relation),
        "frown" => ("⌢", SymbolRole::Relation),
        // arrows (relation role)
        "leftarrow" => ("←", SymbolRole::Relation),
        "gets" => ("←", SymbolRole::Relation),
        "rightarrow" => ("→", SymbolRole::Relation),
        "to" => ("→", SymbolRole::Relation),
        "leftrightarrow" => ("↔", SymbolRole::Relation),
        "Leftarrow" => ("⇐", SymbolRole::Relation),
        "Rightarrow" => ("⇒", SymbolRole::Relation),
        "Leftrightarrow" => ("⇔", SymbolRole::Relation),
        "mapsto" => ("↦", SymbolRole::Relation),
        "hookleftarrow" => ("↩", SymbolRole::Relation),
        "hookrightarrow" => ("↪", SymbolRole::Relation),
        "leftharpoonup" => ("↼", SymbolRole::Relation),
        "leftharpoondown" => ("↽", SymbolRole::Relation),
        "rightharpoonup" => ("⇀", SymbolRole::Relation),
        "rightharpoondown" => ("⇁", SymbolRole::Relation),
        "rightleftharpoons" => ("⇌", SymbolRole::Relation),
        "longleftarrow" => ("⟵", SymbolRole::Relation),
        "longrightarrow" => ("⟶", SymbolRole::Relation),
        "longleftrightarrow" => ("⟷", SymbolRole::Relation),
        "Longleftarrow" => ("⟸", SymbolRole::Relation),
        "Longrightarrow" => ("⟹", SymbolRole::Relation),
        "Longleftrightarrow" => ("⟺", SymbolRole::Relation),
        "longmapsto" => ("⟼", SymbolRole::Relation),
        "uparrow" => ("↑", SymbolRole::Relation),
        "downarrow" => ("↓", SymbolRole::Relation),
        "updownarrow" => ("↕", SymbolRole::Relation),
        "Uparrow" => ("⇑", SymbolRole::Relation),
        "Downarrow" => ("⇓", SymbolRole::Relation),
        "Updownarrow" => ("⇕", SymbolRole::Relation),
        "nearrow" => ("↗", SymbolRole::Relation),
        "searrow" => ("↘", SymbolRole::Relation),
        "swarrow" => ("↙", SymbolRole::Relation),
        "nwarrow" => ("↖", SymbolRole::Relation),
        // punctuation
        "cdots" => ("⋯", SymbolRole::Punctuation),
        "ldots" => ("…", SymbolRole::Punctuation),
        "dots" => ("…", SymbolRole::Punctuation),
        "vdots" => ("⋮", SymbolRole::Punctuation),
        "ddots" => ("⋱", SymbolRole::Punctuation),
        "colon" => (":", SymbolRole::Punctuation),
        _ => return None,
    };
    Some(SymbolDefinition { value, role })
}

/// `largeOperators` — `limits: !command.includes("int")`.
pub(crate) fn lookup_large_operator(command: &str) -> Option<(&'static str, bool)> {
    Some(match command {
        "sum" => ("∑", true),
        "prod" => ("∏", true),
        "coprod" => ("∐", true),
        "int" => ("∫", false),
        "iint" => ("∬", false),
        "iiint" => ("∭", false),
        "oint" => ("∮", false),
        "bigcap" => ("⋂", true),
        "bigcup" => ("⋃", true),
        "bigvee" => ("⋁", true),
        "bigwedge" => ("⋀", true),
        "bigoplus" => ("⨁", true),
        "bigotimes" => ("⨂", true),
        "bigodot" => ("⨀", true),
        _ => return None,
    })
}

/// `namedOperators` — `limits: startsWith("lim") || min || max`.
pub(crate) fn lookup_named_operator(command: &str) -> Option<(String, bool)> {
    let known = matches!(
        command,
        "arccos"
            | "arcsin"
            | "arctan"
            | "arg"
            | "cos"
            | "cosh"
            | "cot"
            | "coth"
            | "csc"
            | "deg"
            | "det"
            | "dim"
            | "exp"
            | "gcd"
            | "hom"
            | "inf"
            | "ker"
            | "lg"
            | "lim"
            | "liminf"
            | "limsup"
            | "ln"
            | "log"
            | "max"
            | "min"
            | "mod"
            | "Pr"
            | "sec"
            | "sin"
            | "sinh"
            | "sup"
            | "tan"
            | "tanh"
    );
    if !known {
        return None;
    }
    let limits = command.starts_with("lim") || command == "min" || command == "max";
    Some((command.to_string(), limits))
}

/// `delimiterTable` — keyed by both `\\command` and bare forms.
#[allow(clippy::match_same_arms)]
pub(crate) fn lookup_delimiter(token: &str) -> Option<&'static str> {
    Some(match token {
        "(" => "(",
        ")" => ")",
        "[" => "[",
        "]" => "]",
        "\\{" => "{",
        "\\}" => "}",
        "{" => "{",
        "}" => "}",
        "|" => "│",
        "\\|" => "║",
        "vert" => "│",
        "Vert" => "║",
        "lvert" => "│",
        "rvert" => "│",
        "lVert" => "║",
        "rVert" => "║",
        "lbrace" => "{",
        "rbrace" => "}",
        "langle" => "⟨",
        "rangle" => "⟩",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lceil" => "⌈",
        "rceil" => "⌉",
        "." => "",
        _ => return None,
    })
}

/// `spacingCommands` — the width in terminal cells.
#[allow(clippy::match_same_arms)]
pub(crate) fn lookup_spacing(command: &str) -> Option<u32> {
    Some(match command {
        "," => 0,
        ":" => 1,
        ";" => 1,
        "!" => 0,
        "quad" => 2,
        "qquad" => 4,
        "enspace" => 1,
        "thinspace" => 0,
        _ => return None,
    })
}
