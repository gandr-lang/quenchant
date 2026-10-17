// The grammar's builders, read as source: `RuleName("commented_type")` in a
// comment defines nothing.

/// The rules of a small surface.
pub fn rules() -> Vec<Rule>
{
    let note = "RuleName(\"quoted_type\")";
    vec![
        rule(RuleName("unit_type"), note),
        rule(RuleName("pair_type"), note),
        rule(RuleName("record_type"), note),
        rule(RuleName("shebang"), note),
    ]
}

#[cfg(test)]
mod tests
{
    /// A rule built only by a test is no rule of the surface.
    fn sentinel()
    {
        if true {
            rule(RuleName("sentinel"), "");
        }
    }
}
