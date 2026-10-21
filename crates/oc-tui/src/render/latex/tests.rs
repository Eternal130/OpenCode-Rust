//! Ported upstream tests: `parser.test.ts`, `layout.test.ts`,
//! `render.test.ts` (packages/latex/src/*.test.ts).

use crate::render::latex::layout::{layout_math, RenderLatexOptions};
use crate::render::latex::parser::parse_latex;
use crate::render::latex::render_latex_to_string;
use crate::render::latex::types::{
    BracePosition, MathNode, MatrixEnvironment, NumeratorAlign, ParseOptions, SymbolRole,
};

fn strict() -> ParseOptions {
    ParseOptions { strict: true, ..ParseOptions::default() }
}

fn relaxed() -> ParseOptions {
    ParseOptions::default()
}

fn text(value: &str) -> MathNode {
    MathNode::Text(value.to_string())
}

// -- parser.test.ts ----------------------------------------------------------

#[test]
fn parses_fractions_and_scripts_structurally() {
    let parsed = parse_latex(r"\frac{x^2+1}{y_0}", &relaxed()).expect("parses");
    let MathNode::Fraction { numerator, denominator, bar, .. } = &parsed else {
        panic!("fraction: {parsed:?}");
    };
    assert!(matches!(**numerator, MathNode::Row(_)), "{numerator:?}");
    assert!(matches!(**denominator, MathNode::Scripts { .. }), "{denominator:?}");
    assert!(*bar);
}

#[test]
fn parses_matrix_environments_into_rows_and_cells() {
    let parsed =
        parse_latex(r"\begin{pmatrix}a & b \\ c & d\end{pmatrix}", &relaxed()).expect("parses");
    let MathNode::Matrix { rows, environment, .. } = &parsed else {
        panic!("matrix: {parsed:?}");
    };
    assert_eq!(*environment, MatrixEnvironment::Pmatrix);
    assert_eq!(rows.len(), 2);
    assert!(matches!(rows[0][0], MathNode::Symbol { ref value, .. } if value == "a"));
    assert!(matches!(rows[0][1], MathNode::Symbol { ref value, .. } if value == "b"));
    assert!(matches!(rows[1][0], MathNode::Symbol { ref value, .. } if value == "c"));
    assert!(matches!(rows[1][1], MathNode::Symbol { ref value, .. } if value == "d"));
}

#[test]
fn accepts_array_column_specs_and_starred_alignment_environments() {
    let parsed =
        parse_latex(r"\begin{array}{cc}a & b \\ c & d\end{array}", &relaxed()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Matrix { environment: MatrixEnvironment::Array, columns: Some(columns), rows }
            if columns == "cc" && rows.len() == 2 && rows.iter().all(|row| row.len() == 2)
    ));
    let parsed =
        parse_latex(r"\begin{align*}a &= b \\ c &= d\end{align*}", &relaxed()).expect("parses");
    assert!(matches!(&parsed, MathNode::Matrix { environment: MatrixEnvironment::Align, .. }));
}

#[test]
fn preserves_double_norm_delimiters_without_changing_single_bars() {
    let parsed = parse_latex(r"\|v\|", &strict()).expect("parses");
    let MathNode::Row(body) = &parsed else { panic!("row: {parsed:?}") };
    let values: Vec<&str> = body
        .iter()
        .map(|node| match node {
            MathNode::Symbol { value, .. } => value.as_str(),
            other => panic!("symbol: {other:?}"),
        })
        .collect();
    assert_eq!(values, vec!["║", "v", "║"]);

    let parsed = parse_latex(r"\left\|v\right\|", &strict()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Delimited { left, right, .. } if left == "║" && right == "║"
    ));
    let parsed = parse_latex(r"\left|v\right|", &strict()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Delimited { left, right, .. } if left == "│" && right == "│"
    ));
}

#[test]
fn matches_the_whole_right_command_and_keeps_nested_delimiters() {
    let parsed = parse_latex(r"\left(A\rightarrow B\right)", &strict()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Delimited { left, right, .. } if left == "(" && right == ")"
    ));
    // body: A → B
    let MathNode::Delimited { body, .. } = &parsed else { unreachable!() };
    let MathNode::Row(body) = &**body else { panic!("row body") };
    let values: Vec<&str> = body
        .iter()
        .map(|node| match node {
            MathNode::Symbol { value, .. } => value.as_str(),
            other => panic!("symbol: {other:?}"),
        })
        .collect();
    assert_eq!(values, vec!["A", "→", "B"]);

    let parsed =
        parse_latex(r"\left(\left[A\right]\rightharpoonup B\right)", &strict()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Delimited { body, .. }
            if matches!(&**body, MathNode::Row(body)
                if matches!(&body[0], MathNode::Delimited { .. })
                    && matches!(&body[1], MathNode::Symbol { value, .. } if value == "⇀")
                    && matches!(&body[2], MathNode::Symbol { value, .. } if value == "B"))
    ));

    assert!(parse_latex(r"\left(A\rightarrow B", &strict()).is_err());
    let error = parse_latex(r"\left(A\rightward B\right)", &strict()).unwrap_err();
    assert!(error.message.contains("Unsupported command \\rightward"), "{error}");
}

#[test]
fn accepts_empty_leading_interior_and_trailing_environment_cells() {
    let parsed = parse_latex(r"\begin{aligned}&=x\\&=y\end{aligned}", &strict()).expect("parses");
    let MathNode::Matrix { rows, .. } = &parsed else { panic!() };
    assert_eq!(rows.len(), 2);
    assert!(matches!(&rows[0][0], MathNode::Row(cells) if cells.is_empty()));
    assert!(matches!(&rows[0][1], MathNode::Row(cells) if cells.len() == 2));

    let parsed = parse_latex(r"\begin{matrix}a&&\\&b&\end{matrix}", &strict()).expect("parses");
    let MathNode::Matrix { rows, .. } = &parsed else { panic!() };
    assert!(matches!(&rows[0][0], MathNode::Symbol { value, .. } if value == "a"));
    assert!(matches!(&rows[0][1], MathNode::Row(cells) if cells.is_empty()));
    assert!(matches!(&rows[1][1], MathNode::Symbol { value, .. } if value == "b"));

    let parsed = parse_latex(r"\begin{matrix}a\\\end{matrix}", &relaxed()).expect("parses");
    assert!(
        matches!(&parsed, MathNode::Matrix { rows, .. } if rows.len() == 1 && rows[0].len() == 1)
    );
    let parsed = parse_latex(r"\begin{matrix}\\\end{matrix}", &relaxed()).expect("parses");
    assert!(matches!(&parsed, MathNode::Matrix { rows, .. } if rows.len() == 1));
}

#[test]
fn parses_displaylines_as_separate_gathered_rows() {
    let parsed = parse_latex(r"\displaylines{x=1\\y=2}", &strict()).expect("parses");
    assert!(
        matches!(&parsed, MathNode::Matrix { environment: MatrixEnvironment::Gathered, rows, .. } if rows.len() == 2)
    );
    let parsed = parse_latex(r"\displaylines{\frac{1}{2}\\{y}}+z", &relaxed()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Row(body)
            if matches!(&body[0], MathNode::Matrix { .. })
                && matches!(&body[1], MathNode::Symbol { value, .. } if value == "+")
                && matches!(&body[2], MathNode::Symbol { value, .. } if value == "z")
    ));
    assert!(parse_latex(r"\displaylines[l]{x\\y}", &strict()).is_err());
    assert!(parse_latex(r"\displaylines{x\\y", &strict()).is_err());
}

#[test]
fn parses_continued_fraction_alignment_before_its_arguments() {
    for (option, expected) in [
        ("", None),
        ("[]", None),
        ("[l]", Some(NumeratorAlign::Left)),
        ("[r]", Some(NumeratorAlign::Right)),
    ] {
        let source = format!(r"\cfrac{option}{{1}}{{23}}");
        let parsed =
            parse_latex(&source, &strict()).unwrap_or_else(|error| panic!("{source}: {error}"));
        assert_eq!(
            parsed,
            MathNode::Fraction {
                numerator: Box::new(MathNode::Symbol {
                    value: "1".into(),
                    role: Some(SymbolRole::Ordinary)
                }),
                denominator: Box::new(MathNode::Row(vec![
                    MathNode::Symbol { value: "2".into(), role: Some(SymbolRole::Ordinary) },
                    MathNode::Symbol { value: "3".into(), role: Some(SymbolRole::Ordinary) },
                ])),
                bar: true,
                numerator_align: expected,
            },
            "{source}"
        );
    }
}

#[test]
fn rejects_unsupported_continued_fraction_alignment() {
    for option in ["[c]", "[lr]", "[left]", "[l"] {
        let source = format!(r"\cfrac{option}{{1}}{{2}}");
        assert!(parse_latex(&source, &strict()).is_err(), "{source}");
    }
}

#[test]
fn retains_normalized_array_columns_including_edge_and_double_rules() {
    let parsed =
        parse_latex(r"\begin{array}{ | l || c r | }a&b&c\end{array}", &strict()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Matrix { columns: Some(columns), .. } if columns == "|l||cr|"
    ));
}

#[test]
fn rejects_unsupported_array_columns() {
    for columns in ["", "||", "p{2cm}", "*{2}{c}", "c@{}c", "lXr"] {
        let source = format!(r"\begin{{array}}{{{columns}}}a\end{{array}}");
        assert!(parse_latex(&source, &strict()).is_err(), "{source}");
    }
}

#[test]
fn requires_an_array_column_specification() {
    assert!(parse_latex(r"\begin{array}a&b\end{array}", &strict()).is_err());
}

#[test]
fn emits_structural_braces_while_keeping_annotations_as_scripts() {
    let parsed = parse_latex(r"\overbrace{a+b}^{n}", &strict()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Scripts {
            base,
            superscript: Some(_),
            ..
        } if matches!(&**base, MathNode::Brace { position: BracePosition::Over, body, .. }
            if matches!(&**body, MathNode::Row(_)))
    ));
    let parsed = parse_latex(r"\underbrace{x}_{k}", &strict()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Scripts { base, subscript: Some(_), .. }
            if matches!(&**base, MathNode::Brace { body, position: BracePosition::Under, .. }
                if matches!(&**body, MathNode::Symbol { value, .. } if value == "x"))
    ));
}

#[test]
fn recognizes_named_braces_and_rejects_unsupported_delimiter_commands_in_strict_mode() {
    let parsed = parse_latex(r"\left\lbrace x\right\rbrace", &strict()).expect("parses");
    assert!(
        matches!(&parsed, MathNode::Delimited { left, right, .. } if left == "{" && right == "}")
    );
    let parsed = parse_latex(r"\lbrace x\rbrace", &strict()).expect("parses");
    assert!(matches!(&parsed, MathNode::Row(body) if body.len() == 3));
    for source in [
        r"\left\unknown x\right)",
        r"\left(x\right\unknown",
        r"\big\unknown",
        r"\left(x\middle\unknown y\right)",
    ] {
        let error = parse_latex(source, &strict()).unwrap_err();
        assert!(error.message.contains("Unsupported delimiter \\unknown"), "{source}: {error}");
    }
}

#[test]
fn expands_user_macros() {
    let mut macros = std::collections::HashMap::new();
    macros.insert("\\R".to_string(), r"\mathbb{R}".to_string());
    let parsed = parse_latex(
        r"\R \to \R",
        &ParseOptions { macros: Some(macros), ..ParseOptions::default() },
    )
    .expect("parses");
    assert!(matches!(&parsed, MathNode::Row(_)));
}

#[test]
fn reports_useful_strict_mode_errors() {
    assert!(parse_latex(r"\definitelyUnknown{x}", &strict()).is_err());
}

#[test]
fn keeps_escaped_braces_inside_raw_text_groups() {
    let parsed = parse_latex(r"\text{left \{ only}", &relaxed()).expect("parses");
    assert_eq!(parsed, text("left { only"));
    let parsed = parse_latex(r"\text{right \} only}", &relaxed()).expect("parses");
    assert_eq!(parsed, text("right } only"));
}

#[test]
fn supports_starred_named_operators_and_limits_modifiers() {
    let parsed = parse_latex(r"\operatorname*{arg\,max}_{x}", &relaxed()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Scripts { base, subscript: Some(_), .. }
            if matches!(&**base, MathNode::Operator { value, limits: true, .. } if value == "arg max")
    ));
    let parsed = parse_latex(r"\int\limits_0^1", &relaxed()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Scripts { base, .. } if matches!(&**base, MathNode::Operator { value, limits: true, .. } if value == "∫")
    ));
    let parsed = parse_latex(r"\sum\nolimits_{i=1}", &relaxed()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Scripts { base, .. } if matches!(&**base, MathNode::Operator { value, limits: false, .. } if value == "∑")
    ));
}

#[test]
fn interprets_operator_spacing_and_preserves_roman_text_whitespace() {
    let parsed = parse_latex(r"\operatorname{arg\,max}", &strict()).expect("parses");
    assert_eq!(parsed, MathNode::Operator { value: "arg max".into(), limits: false });
    let parsed = parse_latex(r"\textrm{ if }", &strict()).expect("parses");
    assert!(matches!(
        &parsed,
        MathNode::Variant { body, .. } if matches!(&**body, MathNode::Text(value) if value == " if ")
    ));
}

#[test]
fn bounds_source_and_recursive_macro_expansion() {
    let error = parse_latex(
        "12345",
        &ParseOptions { max_source_length: Some(4), ..ParseOptions::default() },
    )
    .unwrap_err();
    assert!(error.message.contains("4-character limit"), "{error}");

    let mut macros = std::collections::HashMap::new();
    macros.insert("a".to_string(), r"\a\a".to_string());
    let error = parse_latex(
        r"\a",
        &ParseOptions {
            macros: Some(macros),
            max_expanded_length: Some(64),
            ..ParseOptions::default()
        },
    )
    .unwrap_err();
    assert!(error.message.contains("64-character limit"), "{error}");

    let mut macros = std::collections::HashMap::new();
    macros.insert("a".to_string(), "{{x}}".to_string());
    let error = parse_latex(
        r"\a",
        &ParseOptions { macros: Some(macros), max_depth: Some(1), ..ParseOptions::default() },
    )
    .unwrap_err();
    assert!(error.message.contains("1-level limit"), "{error}");

    assert!(parse_latex(
        "x",
        &ParseOptions { max_source_length: Some(0), ..ParseOptions::default() }
    )
    .is_err());
}

#[test]
fn fails_quickly_when_malformed_environments_cannot_advance() {
    let error = parse_latex(r"\begin{matrix}]", &relaxed()).unwrap_err();
    assert!(error.message.contains("Missing \\end{matrix}"), "{error}");
    let error = parse_latex(r"\begin{matrix}x}", &relaxed()).unwrap_err();
    assert!(error.message.contains("Unexpected \"}\" in matrix"), "{error}");
    let error = parse_latex(r"\begin{matrix}&}", &relaxed()).unwrap_err();
    assert!(error.message.contains("Unexpected \"}\" in matrix"), "{error}");
}

#[test]
fn bounds_structural_nesting_with_a_parse_error_instead_of_overflowing_the_stack() {
    let source = format!("{}x{}", "{".repeat(80), "}".repeat(80));
    let error =
        parse_latex(&source, &ParseOptions { max_depth: Some(64), ..ParseOptions::default() })
            .unwrap_err();
    assert!(error.message.contains("64-level limit"), "{error}");
    let source = format!("{}x", r"\frac".repeat(80));
    let error =
        parse_latex(&source, &ParseOptions { max_depth: Some(64), ..ParseOptions::default() })
            .unwrap_err();
    assert!(error.message.contains("64-level limit"), "{error}");
}

// -- layout.test.ts ----------------------------------------------------------

fn layout_options() -> RenderLatexOptions {
    RenderLatexOptions::default()
}

#[test]
fn empty_scripts_do_not_change_geometry() {
    for source in [
        r"\sqrt{x}",
        r"\begin{pmatrix}a&b\\c&d\end{pmatrix}",
        r"\underbrace{abcd}",
        r"\overbrace{abcd}",
        r"\sum",
    ] {
        let plain = render_latex_to_string(source, &layout_options()).expect("plain");
        for scripts in ["^{}", "_{}", "^{}_{}"] {
            let with = render_latex_to_string(&format!("{source}{scripts}"), &layout_options())
                .unwrap_or_else(|error| panic!("{source}{scripts}: {error}"));
            assert_eq!(with, plain, "{source}{scripts}");
        }
    }
}

#[test]
fn centers_annotations_over_even_width_brace_junctions() {
    assert_eq!(
        render_latex_to_string(r"\overbrace{abcd}^{n}", &layout_options()).unwrap(),
        [" n", "╭┴─╮", "abcd"].join("\n")
    );
    assert_eq!(
        render_latex_to_string(r"\underbrace{abcd}_{n}", &layout_options()).unwrap(),
        ["abcd", "╰┬─╯", " n"].join("\n")
    );
}

#[test]
fn raises_powers_above_tall_matrix_delimiters() {
    assert_eq!(
        render_latex_to_string(r"\begin{pmatrix}a&b\\c&d\end{pmatrix}^2", &layout_options())
            .unwrap(),
        ["     2", "⎛a b⎞", "⎜   ⎟", "⎝c d⎠"].join("\n")
    );
}

#[test]
fn keeps_piecewise_values_left_aligned() {
    assert_eq!(
        render_latex_to_string(
            r"\begin{cases}x & x>0\\x^2+1 & x\le0\end{cases}",
            &layout_options()
        )
        .unwrap(),
        ["⎧x       x > 0", "⎨", "⎩x² + 1  x ≤ 0"].join("\n")
    );
}

#[test]
fn honors_array_column_alignment_and_continuous_separators() {
    let node = MathNode::Matrix {
        rows: vec![vec![text("a"), text("wide")], vec![text("long"), text("b")]],
        environment: MatrixEnvironment::Array,
        columns: Some("l|r".to_string()),
    };
    assert_eq!(
        layout_math(&node, &layout_options()).to_string(),
        ["a    │ wide", "     │", "long │    b"].join("\n")
    );
}

#[test]
fn preserves_edge_rules_and_double_array_separators() {
    let node = MathNode::Matrix {
        rows: vec![vec![text("a"), text("b")], vec![text("long"), text("c")]],
        environment: MatrixEnvironment::Array,
        columns: Some("|l||r|".to_string()),
    };
    assert_eq!(
        layout_math(&node, &layout_options()).to_string(),
        ["│ a    ││ b │", "│      ││   │", "│ long ││ c │"].join("\n")
    );
}

#[test]
fn aligns_continued_fraction_numerators() {
    for (align, numerator_row) in [(NumeratorAlign::Left, " 1"), (NumeratorAlign::Right, "     1")]
    {
        let node = MathNode::Fraction {
            numerator: Box::new(text("1")),
            denominator: Box::new(text("12345")),
            bar: true,
            numerator_align: Some(align),
        };
        assert_eq!(
            layout_math(&node, &layout_options()).to_string(),
            [numerator_row, "───────", " 12345"].join("\n")
        );
    }
}

#[test]
fn stretches_braces_and_places_annotations_outside_them() {
    for position in [BracePosition::Over, BracePosition::Under] {
        let over = position == BracePosition::Over;
        let node = MathNode::Scripts {
            base: Box::new(MathNode::Brace { body: Box::new(text("a + b + c")), position }),
            superscript: over.then(|| Box::new(text("n"))),
            subscript: (!over).then(|| Box::new(text("n"))),
        };
        let layout = layout_math(&node, &layout_options());
        assert_eq!(
            layout.to_string(),
            if over {
                ["    n", "╭───┴───╮", "a + b + c"].join("\n")
            } else {
                ["a + b + c", "╰───┬───╯", "    n"].join("\n")
            }
        );
        assert_eq!(layout.baseline, if over { 2 } else { 0 });
    }
}

// -- render.test.ts ----------------------------------------------------------

#[test]
fn renders_a_fraction_with_a_centered_rule() {
    assert_eq!(
        render_latex_to_string(r"\frac{x+1}{y-1}", &RenderLatexOptions::default()).unwrap(),
        [" x + 1", "───────", " y - 1"].join("\n")
    );
}

#[test]
fn compacts_scripts_only_when_every_script_is_supported() {
    for (source, expected) in [
        (r"E = mc^2", "E = mc²"),
        (r"a_n", "aₙ"),
        (r"x_i^2", "x²ᵢ"),
        (r"x^{}", "x"),
        (r"x_{}", "x"),
        (r"x^{}_{}", "x"),
        (r"x^m_1", " m\nx\n 1"),
        (r"x^2_q", " 2\nx\n q"),
        (r"x^{\frac{1}{2}}_1", "  1\n ───\n  2\nx\n 1"),
    ] {
        assert_eq!(
            render_latex_to_string(source, &RenderLatexOptions::default()).unwrap(),
            expected,
            "{source}"
        );
    }
}

#[test]
fn respects_script_and_display_mode_options() {
    let options =
        RenderLatexOptions { compact_scripts: Some(false), ..RenderLatexOptions::default() };
    assert_eq!(render_latex_to_string(r"x_i^2", &options).unwrap(), " 2\nx\n i");
    let options = RenderLatexOptions { display_mode: Some(false), ..RenderLatexOptions::default() };
    assert_eq!(render_latex_to_string(r"\sum_1^n", &options).unwrap(), "∑ⁿ₁");
    let options =
        RenderLatexOptions { compact_scripts: Some(false), ..RenderLatexOptions::default() };
    assert_eq!(render_latex_to_string(r"\sum_1^n", &options).unwrap(), "n\n∑\n1");
}

#[test]
fn centers_binomials_around_an_empty_math_axis_row() {
    assert_eq!(
        render_latex_to_string(r"P = \binom{n}{k}", &RenderLatexOptions::default()).unwrap(),
        ["    ⎛ n ⎞", "P = ⎜   ⎟", "    ⎝ k ⎠"].join("\n")
    );
}

#[test]
fn renders_roots_with_a_vinculum() {
    assert_eq!(
        render_latex_to_string(r"\sqrt{x^2+y^2}", &RenderLatexOptions::default()).unwrap(),
        [" ╭───────", "╰╯x² + y²"].join("\n")
    );
}

#[test]
fn renders_matrices_with_stretching_delimiters() {
    assert_eq!(
        render_latex_to_string(
            r"\begin{pmatrix}a & b \\ c & d\end{pmatrix}",
            &RenderLatexOptions::default()
        )
        .unwrap(),
        ["⎛a b⎞", "⎜   ⎟", "⎝c d⎠"].join("\n")
    );
}

#[test]
fn places_display_operator_limits_above_and_below() {
    assert_eq!(
        render_latex_to_string(r"\sum_{i=1}^{n} i^2", &RenderLatexOptions::default()).unwrap(),
        ["  n", "  ∑   i²", "i = 1"].join("\n")
    );
}

#[test]
fn returns_intrinsic_geometry_and_baseline() {
    let layout =
        crate::render::latex::render_latex(r"\frac{1}{2}", &RenderLatexOptions::default()).unwrap();
    assert_eq!(layout.width, 3);
    assert_eq!(layout.height, 3);
    assert_eq!(layout.baseline, 1);
}

#[test]
fn renders_blackboard_calligraphic_and_fraktur_alphabets() {
    assert_eq!(
        render_latex_to_string(
            r"\mathbb{R} \to \mathcal{C} \times \mathfrak{g}",
            &RenderLatexOptions::default()
        )
        .unwrap(),
        "ℝ → 𝒞 × 𝔤"
    );
}

#[test]
fn preserves_inherited_styles_through_nested_variants_and_colors() {
    let layout = crate::render::latex::render_latex(
        r"\mathbf{\mathsf{\textcolor{red}{\mathit{x}}}}",
        &RenderLatexOptions::default(),
    )
    .unwrap();
    let cell = layout.cells[0][0].clone().expect("cell");
    assert_eq!(cell.char, "x");
    let style = cell.style.expect("style");
    assert!(style.bold);
    assert!(style.italic);
    assert_eq!(style.color.as_deref(), Some("red"));
}

#[test]
fn renders_nested_fractions_without_flattening_their_structure() {
    let result =
        render_latex_to_string(r"\frac{1}{1+\frac{1}{x}}", &RenderLatexOptions::default()).unwrap();
    assert_eq!(result.split('\n').count(), 5);
    assert!(result.matches('─').count() >= 10);
}

#[test]
fn renders_common_textbook_structures() {
    let result = render_latex_to_string(
        r"\left[\frac{-b \pm \sqrt{b^2-4ac}}{2a}\right]",
        &RenderLatexOptions::default(),
    )
    .unwrap();
    assert!(result.contains('±'));
    assert!(result.contains("╰╯"));
    assert!(result.contains('─'));
    assert!(result.contains('⎡'));
    assert!(result.contains('⎦'));
}

#[test]
fn places_fallback_combining_negation_after_the_base_symbol() {
    let result =
        render_latex_to_string(r"\not\rightarrow", &RenderLatexOptions::default()).unwrap();
    let chars: Vec<char> = result.chars().collect();
    assert_eq!(chars, vec!['→', '̸']);
}

#[test]
fn treats_square_brackets_as_ordinary_interval_delimiters() {
    assert_eq!(
        render_latex_to_string(r"x\in[0,1]", &RenderLatexOptions::default()).unwrap(),
        "x ∈ [0,1]"
    );
    assert_eq!(
        render_latex_to_string(r"[-1,1]", &RenderLatexOptions::default()).unwrap(),
        "[-1,1]"
    );
}
