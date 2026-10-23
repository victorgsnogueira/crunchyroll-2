//! Language names in Portuguese.

/// Human name for a Crunchyroll locale code like "pt-BR".
pub fn language_name(code: &str) -> String {
    let name = match code {
        "ja-JP" => "Japonês",
        "pt-BR" => "Português (Brasil)",
        "pt-PT" => "Português (Portugal)",
        "en-US" => "Inglês",
        "en-IN" => "Inglês (Índia)",
        "es-419" => "Espanhol (América Latina)",
        "es-ES" => "Espanhol (Espanha)",
        "fr-FR" => "Francês",
        "de-DE" => "Alemão",
        "it-IT" => "Italiano",
        "ru-RU" => "Russo",
        "ar-SA" => "Árabe",
        "hi-IN" => "Hindi",
        "ta-IN" => "Tâmil",
        "te-IN" => "Telugu",
        "ko-KR" => "Coreano",
        "zh-CN" => "Chinês (simplificado)",
        "zh-TW" => "Chinês (tradicional)",
        "zh-HK" => "Chinês (Hong Kong)",
        "th-TH" => "Tailandês",
        "id-ID" => "Indonésio",
        "ms-MY" => "Malaio",
        "vi-VN" => "Vietnamita",
        "tr-TR" => "Turco",
        "pl-PL" => "Polonês",
        "ca-ES" => "Catalão",
        "tl-PH" => "Filipino",
        "" => "Desconhecido",
        other => return other.to_string(),
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(language_name("ja-JP"), "Japonês");
        assert_eq!(language_name("pt-BR"), "Português (Brasil)");
        assert_eq!(language_name("xx-YY"), "xx-YY");
    }
}
