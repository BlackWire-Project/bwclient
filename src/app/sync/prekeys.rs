use super::*;

impl App {
    pub(crate) fn replenish_prekeys_if_needed(&mut self) -> Result<()> {
        let Some(profile) = self.active_profile.clone() else {
            return Ok(());
        };
        if available_prekeys(&profile.keys) >= 5 {
            return Ok(());
        }
        let Some(server) = self.active_server.clone() else {
            return Ok(());
        };
        let relay = RelayClient::new(server.base_url.clone())?;
        let mut profile = profile;
        let new_prekeys = generate_more_prekeys(12);
        let public_keys: Vec<String> = new_prekeys
            .iter()
            .map(|prekey| prekey.public_key.clone())
            .collect();
        relay.add_prekeys(&profile.username, &public_keys)?;
        profile.keys.one_time_prekeys.extend(new_prekeys);
        self.storage
            .update_profile_keys(profile.id, &profile.keys)?;
        self.active_profile = Some(profile);
        self.status = "Uploaded more one-time prekeys.".to_string();
        Ok(())
    }
}
