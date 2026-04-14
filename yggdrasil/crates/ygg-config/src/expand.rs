//! Environment variable expansion for config strings.
//!
//! Replaces `${VAR_NAME}` patterns with their environment variable values.

/// Expand all `${VAR_NAME}` placeholders in a string with environment variable values.
///
/// If an environment variable is not set, the placeholder is left as-is and a
/// warning is logged. Supports multiple placeholders per string.
///
/// # Examples
/// ```
/// unsafe { std::env::set_var("MY_HOST", "localhost") };
/// let result = ygg_config::expand_env_vars("http://${MY_HOST}:8080");
/// assert_eq!(result, "http://localhost:8080");
/// unsafe { std::env::remove_var("MY_HOST") };
/// ```
pub fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            // Consume the '{'
            chars.next();

            // Read the variable name until '}'
            let mut var_name = String::new();
            let mut found_close = false;
            for ch in chars.by_ref() {
                if ch == '}' {
                    found_close = true;
                    break;
                }
                var_name.push(ch);
            }

            if found_close && !var_name.is_empty() {
                match std::env::var(&var_name) {
                    Ok(val) => result.push_str(&val),
                    Err(_) => {
                        tracing::warn!(
                            var = %var_name,
                            "environment variable not set, leaving placeholder"
                        );
                        // Leave placeholder as-is
                        result.push_str("${");
                        result.push_str(&var_name);
                        result.push('}');
                    }
                }
            } else {
                // Malformed placeholder — emit literally
                result.push_str("${");
                result.push_str(&var_name);
                if !found_close {
                    // No closing brace found
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}
