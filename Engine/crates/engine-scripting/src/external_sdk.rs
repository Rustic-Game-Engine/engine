use crate::ScriptLanguage;

pub(crate) fn files(language: ScriptLanguage) -> &'static [(&'static str, &'static str)] {
    match language {
        ScriptLanguage::Python => &[("rustic.py", include_str!("sdk/rustic.py"))],
        ScriptLanguage::CSharp => &[("Rustic.cs", include_str!("sdk/Rustic.cs"))],
        ScriptLanguage::Php => &[("rustic.php", include_str!("sdk/rustic.php"))],
        ScriptLanguage::Java => &[("Rustic.java", include_str!("sdk/Rustic.java"))],
        ScriptLanguage::C => &[("rustic.h", include_str!("sdk/rustic.h"))],
        _ => &[],
    }
}
