// The grammar's builders, read as source.

/// The rules of a small surface.
pub fn rules() -> Vec<Rule>
{
    vec![
        rule(RuleName("unit_type")),
        rule(RuleName("pair_type")),
        rule(RuleName("record_type")),
        rule(RuleName("shebang")),
    ]
}

#[cfg(test)]
mod tests
{
    /// A rule built only by a test is no rule of the surface.
    fn sentinel()
    {
        rule(RuleName("sentinel"));
    }
}
