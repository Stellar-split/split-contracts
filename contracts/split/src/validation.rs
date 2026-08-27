use soroban_sdk::{Address, Env};

pub fn assert_unique_recipients(env: &Env, recipients: &soroban_sdk::Vec<Address>) -> Result<(), String> {
    if recipients.is_empty() {
        return Ok(());
    }

    for i in 0..recipients.len() {
        for j in (i + 1)..recipients.len() {
            if recipients.get(i as u32) == recipients.get(j as u32) {
                return Err("duplicate recipient found".to_string());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_recipient_list_passes_uniqueness_check() {
        let env = Env::default();
        let recipients = soroban_sdk::Vec::<Address>::new(&env);

        let result = assert_unique_recipients(&env, &recipients);
        assert!(result.is_ok(), "empty recipient list should pass uniqueness check");
    }
}
