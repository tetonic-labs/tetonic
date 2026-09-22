#[cfg(test)]
mod tests {
    use crate::scanner::ScannerEngine;
    use std::sync::Arc;
    use tetonic_domain::secrets::SecretScanner;

    struct Fixture {
        text: &'static str,
        path: Option<&'static str>,
        expect_secret: bool,
    }

    fn fixtures() -> Vec<Fixture> {
        vec![
            // True positives - pem-private-key
            Fixture { text: "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----", path: None, expect_secret: true },
            Fixture { text: "Here is my key:\n-----BEGIN PGP PRIVATE KEY-----\nrandomdata\n-----END PGP PRIVATE KEY-----", path: None, expect_secret: true },

            // True positives - aws-access-key
            Fixture { text: "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE", path: None, expect_secret: true },
            Fixture { text: "export AWS_ACCESS_KEY=ASIAIOSFODNN7EXAMPLE", path: None, expect_secret: true },

            // True positives - github-token
            Fixture { text: "ghp_1234567890abcdefghijklmnopqrstuvwxyz", path: None, expect_secret: true },
            Fixture { text: "github_token: gho_1234567890abcdefghijklmnopqrstuvwxyz", path: None, expect_secret: true },

            // True positives - env-file
            Fixture { text: "DB_PASS=secret", path: Some(".env"), expect_secret: true },
            Fixture { text: "DB_PASS=secret", path: Some(".env.production"), expect_secret: true },
            Fixture { text: "DB_PASS=secret", path: Some("config/.env.local"), expect_secret: true },

            // True positives - binary-archive
            Fixture { text: "PK\x03\x04...", path: Some("archive.zip"), expect_secret: true },
            Fixture { text: "GZIP binary data", path: Some("backup.tar.gz"), expect_secret: true },

            // True positives - high-entropy
            Fixture { text: "my_secret_token = 'aB3!k9#mPq2@zX8$vL5^wN0'", path: None, expect_secret: true },

            // False positives (Should be false)
            Fixture { text: "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG...\n-----END PUBLIC KEY-----", path: None, expect_secret: false },
            Fixture { text: "AWS_ACCESS_KEY_ID=YOUR_ACCESS_KEY_HERE", path: None, expect_secret: false },
            Fixture { text: "ghp_placeholder_for_docs", path: None, expect_secret: false },
            Fixture { text: "DB_PASS=secret", path: Some("config.env.example"), expect_secret: false }, // Doesn't end in .env
            Fixture { text: "The quick brown fox jumps over the lazy dog", path: None, expect_secret: false },
            Fixture { text: "PK\x03\x04...", path: Some("archive.txt"), expect_secret: false }, // Not a binary archive extension

            // Adding more to reach around 25 for now to represent the evaluation
            Fixture { text: "AKIA1234567890ABCDEF", path: None, expect_secret: true },
            Fixture { text: "ghs_1234567890abcdefghijklmnopqrstuvwxyz", path: None, expect_secret: true },
            Fixture { text: "ghr_1234567890abcdefghijklmnopqrstuvwxyz", path: None, expect_secret: true },
            Fixture { text: "A3T1234567890ABCDEF", path: None, expect_secret: true },
            Fixture { text: "AROA1234567890ABCDEF", path: None, expect_secret: true },
            Fixture { text: "AIPA1234567890ABCDEF", path: None, expect_secret: true },
            Fixture { text: "ANPA1234567890ABCDEF", path: None, expect_secret: true },
            Fixture { text: "ANVA1234567890ABCDEF", path: None, expect_secret: true },
            Fixture { text: "DB_PASS=secret", path: Some("src/.env"), expect_secret: true },
            Fixture { text: "DB_PASS=secret", path: Some("src/.env.test"), expect_secret: true },
            Fixture { text: "DB_PASS=secret", path: Some("src/.env.dev"), expect_secret: true },
            Fixture { text: "DB_PASS=secret", path: Some("src/.env.staging"), expect_secret: true },
            Fixture { text: "DB_PASS=secret", path: Some(".env.production.local"), expect_secret: true },
            Fixture { text: "DB_PASS=secret", path: Some(".env.test.local"), expect_secret: true },
            Fixture { text: "DB_PASS=secret", path: Some(".env.development.local"), expect_secret: true },

            Fixture { text: "DB_PASS=secret", path: Some("env.js"), expect_secret: false },
            Fixture { text: "DB_PASS=secret", path: Some("env.ts"), expect_secret: false },
            Fixture { text: "DB_PASS=secret", path: Some("dotenv.config.js"), expect_secret: false },
        ]
    }

    #[tokio::test]
    async fn evaluate_precision_recall() {
        let scanner = Arc::new(ScannerEngine::default_engine());
        let mut true_positives = 0;
        let mut false_positives = 0;
        let mut true_negatives = 0;
        let mut false_negatives = 0;

        let fix = fixtures();
        let total = fix.len();

        for fixture in fix {
            let res = scanner
                .scan_and_redact(fixture.text, fixture.path)
                .await
                .unwrap();
            let detected = res.is_some() && !res.as_ref().unwrap().0.is_empty();

            match (detected, fixture.expect_secret) {
                (true, true) => true_positives += 1,
                (true, false) => false_positives += 1,
                (false, false) => true_negatives += 1,
                (false, true) => false_negatives += 1,
            }
        }

        let precision = if true_positives + false_positives > 0 {
            true_positives as f64 / (true_positives + false_positives) as f64
        } else {
            1.0
        };

        let recall = if true_positives + false_negatives > 0 {
            true_positives as f64 / (true_positives + false_negatives) as f64
        } else {
            1.0
        };

        println!("Evaluation Results:");
        println!("Total fixtures: {}", total);
        println!("True Positives: {}", true_positives);
        println!("False Positives: {}", false_positives);
        println!("True Negatives: {}", true_negatives);
        println!("False Negatives: {}", false_negatives);
        println!("Precision: {:.2}%", precision * 100.0);
        println!("Recall: {:.2}%", recall * 100.0);

        assert!(precision > 0.8, "Precision too low: {}", precision);
        assert!(recall > 0.8, "Recall too low: {}", recall);
    }
}
