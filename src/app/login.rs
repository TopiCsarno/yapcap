use super::{
    AccountSelectionStatus, AppModel, ClaudeLoginEvent, ClaudeLoginStatus, CodexLoginEvent,
    CodexLoginState, CodexLoginStatus, CursorScanResult, CursorScanState, GeminiLoginEvent,
    GeminiLoginState, GeminiLoginStatus, ManagedClaudeAccountConfig, ManagedCodexAccountConfig,
    ManagedCursorAccountConfig, Message, ProviderAccountRuntimeState, ProviderHealth, ProviderId,
    Task, claude, codex, cursor, gemini, refresh_provider_task, refresh_provider_tasks, runtime,
};

impl AppModel {
    pub(super) fn reauthenticate_codex_account(&mut self, account_id: &str) -> Task<Message> {
        if self
            .config
            .codex_managed_accounts
            .iter()
            .all(|a| a.id != account_id)
        {
            return Task::none();
        }
        self.start_codex_login()
    }

    pub(super) fn start_codex_login(&mut self) -> Task<Message> {
        if self
            .codex_login
            .as_ref()
            .is_some_and(|login| login.status == CodexLoginStatus::Running)
        {
            return Task::none();
        }
        self.codex_login = None;
        let (state, task) = match codex::prepare(self.config.clone()) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.codex_login = Some(CodexLoginState {
                    flow_id: "failed".to_string(),
                    status: CodexLoginStatus::Failed,
                    login_url: None,
                    output: Vec::new(),
                    error: Some(error),
                });
                return Task::none();
            }
        };
        self.start_codex_login_task(state, task)
    }

    pub(super) fn start_codex_login_task(
        &mut self,
        state: CodexLoginState,
        task: cosmic::iced::Task<CodexLoginEvent>,
    ) -> Task<Message> {
        self.codex_login = Some(state);
        let task = task.map(|event| cosmic::Action::App(Message::CodexLoginEvent(Box::new(event))));
        let (task, handle) = task.abortable();
        self.codex_login_handle = Some(handle);
        task
    }

    pub(super) fn cancel_codex_login(&mut self) {
        if let Some(handle) = self.codex_login_handle.take() {
            handle.abort();
        }
        self.codex_login = None;
    }

    pub(super) fn handle_codex_login_event(&mut self, event: CodexLoginEvent) -> Task<Message> {
        match event {
            CodexLoginEvent::Output {
                flow_id,
                line,
                login_url,
            } => {
                let Some(login) = self.codex_login.as_mut() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                if let Some(url) = login_url {
                    login.login_url = Some(url);
                }
                login.output.push(line);
                if login.output.len() > 8 {
                    login.output.remove(0);
                }
                Task::none()
            }
            CodexLoginEvent::Finished { flow_id, result } => {
                let Some(login) = self.codex_login.as_mut() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                self.codex_login_handle = None;
                match *result {
                    Ok(success) => {
                        login.status = CodexLoginStatus::Succeeded;
                        login.error = None;
                        let account_id = success.account.id.clone();
                        let account_label = success.account.label.clone();
                        self.write_config(|new_config| {
                            codex::apply_login_account(new_config, success.account.clone());
                        });
                        runtime::reconcile_provider(
                            &self.config,
                            &mut self.state,
                            ProviderId::Codex,
                        );
                        let mut account = ProviderAccountRuntimeState::empty(
                            ProviderId::Codex,
                            account_id.clone(),
                            account_label,
                        );
                        if let Some(snapshot) = success.snapshot {
                            account.source_label = Some(snapshot.source.clone());
                            account.last_success_at = Some(chrono::Utc::now());
                            account.health = crate::model::ProviderHealth::Ok;
                            account.snapshot = Some(snapshot);
                        }
                        account.auth_state = crate::model::AuthState::Ready;
                        account.error = None;
                        let refresh_succeeded =
                            account.health == ProviderHealth::Ok && account.snapshot.is_some();
                        self.state.upsert_account(account);
                        if let Some(provider) = self.state.provider_mut(ProviderId::Codex) {
                            if !provider.selected_account_ids.contains(&account_id) {
                                provider.selected_account_ids.push(account_id);
                            }
                            provider.account_status = AccountSelectionStatus::Ready;
                            provider.error = None;
                            if refresh_succeeded {
                                provider.legacy_display_snapshot = None;
                            }
                        }
                        runtime::persist_state(&self.state);
                        refresh_provider_tasks(&self.config, &mut self.state)
                    }
                    Err(error) => {
                        login.status = CodexLoginStatus::Failed;
                        login.error = Some(error);
                        Task::none()
                    }
                }
            }
        }
    }

    pub(super) fn update_codex_metadata_from_state(&mut self) {
        let updates = self
            .state
            .accounts_for(ProviderId::Codex)
            .into_iter()
            .filter_map(codex_managed_metadata_update)
            .collect::<Vec<_>>();
        if updates.is_empty() {
            return;
        }

        self.write_config(|new_config| {
            for update in &updates {
                if let Some(account) = new_config
                    .codex_managed_accounts
                    .iter_mut()
                    .find(|account| account.id == update.id)
                {
                    apply_codex_metadata_update(account, update);
                }
            }
        });
        runtime::reconcile_provider(&self.config, &mut self.state, ProviderId::Codex);
    }

    pub(super) fn clear_codex_legacy_snapshot_after_success(&mut self) {
        let active_ok = self
            .state
            .active_account(ProviderId::Codex)
            .is_some_and(|account| {
                account.health == ProviderHealth::Ok && account.snapshot.is_some()
            });
        if !active_ok {
            return;
        }
        if let Some(provider) = self.state.provider_mut(ProviderId::Codex) {
            provider.legacy_display_snapshot = None;
        }
    }

    pub(super) fn update_claude_metadata_from_state(&mut self) {
        let updates = self
            .state
            .accounts_for(ProviderId::Claude)
            .into_iter()
            .filter_map(claude_managed_metadata_update)
            .collect::<Vec<_>>();
        if updates.is_empty() {
            return;
        }

        self.write_config(|new_config| {
            for update in &updates {
                if let Some(account) = new_config
                    .claude_managed_accounts
                    .iter_mut()
                    .find(|account| account.id == update.id)
                {
                    apply_claude_metadata_update(account, update);
                }
            }
        });
        runtime::reconcile_provider(&self.config, &mut self.state, ProviderId::Claude);
    }

    pub(super) fn clear_claude_legacy_snapshot_after_success(&mut self) {
        let active_ok = self
            .state
            .active_account(ProviderId::Claude)
            .is_some_and(|account| {
                account.health == ProviderHealth::Ok && account.snapshot.is_some()
            });
        if !active_ok {
            return;
        }
        if let Some(provider) = self.state.provider_mut(ProviderId::Claude) {
            provider.legacy_display_snapshot = None;
        }
    }

    pub(super) fn update_cursor_metadata_from_state(&mut self) {
        let updates = self
            .state
            .accounts_for(ProviderId::Cursor)
            .into_iter()
            .filter_map(cursor_managed_metadata_update)
            .collect::<Vec<_>>();
        if updates.is_empty() {
            return;
        }
        self.write_config(|new_config| {
            for update in &updates {
                if let Some(account) = new_config.cursor_managed_accounts.iter_mut().find(|a| {
                    (!a.id.is_empty() && a.id == update.config_id) || a.email == update.config_id
                }) {
                    apply_cursor_metadata_update(account, update);
                }
            }
        });
        runtime::reconcile_provider(&self.config, &mut self.state, ProviderId::Cursor);
    }

    pub(super) fn update_cursor_active_account(&mut self) {
        if let Some(provider_state) = self.state.provider_mut(ProviderId::Cursor) {
            provider_state.active_account_id = provider_state.selected_account_ids.first().cloned();
        }
    }

    pub(super) fn start_claude_login(&mut self) -> Task<Message> {
        if self
            .claude_login
            .as_ref()
            .is_some_and(|login| login.status == ClaudeLoginStatus::Running)
        {
            return Task::none();
        }
        self.claude_login = None;
        let state = claude::prepare();
        self.claude_login = Some(state);
        Task::none()
    }

    pub(super) fn reauthenticate_claude_account(&mut self, account_id: &str) -> Task<Message> {
        if self
            .config
            .claude_managed_accounts
            .iter()
            .all(|a| a.id != account_id)
        {
            return Task::none();
        }
        if self
            .claude_login
            .as_ref()
            .is_some_and(|login| login.status == ClaudeLoginStatus::Running)
        {
            return Task::none();
        }
        self.claude_login = None;
        let state = claude::prepare_targeted(account_id.to_string());
        self.claude_login = Some(state);
        Task::none()
    }

    pub(super) fn update_claude_login_code(&mut self, code: String) {
        if let Some(login) = self.claude_login.as_mut()
            && login.status == ClaudeLoginStatus::Running
        {
            login.code_input = code;
        }
    }

    pub(super) fn submit_claude_login_code(&mut self) -> Task<Message> {
        let Some(login) = self.claude_login.as_mut() else {
            return Task::none();
        };
        if login.status != ClaudeLoginStatus::Running || login.code_input.trim().is_empty() {
            return Task::none();
        }
        login.error = None;
        login.output.push("Completing Claude sign-in".to_string());
        if login.output.len() > 8 {
            login.output.remove(0);
        }
        let task = claude::submit_code(login, self.config.clone());
        let task =
            task.map(|event| cosmic::Action::App(Message::ClaudeLoginEvent(Box::new(event))));
        let (task, handle) = task.abortable();
        self.claude_login_handle = Some(handle);
        task
    }

    pub(super) fn cancel_claude_login(&mut self) {
        if let Some(handle) = self.claude_login_handle.take() {
            handle.abort();
        }
        self.claude_login = None;
    }

    pub(super) fn handle_claude_login_event(&mut self, event: ClaudeLoginEvent) -> Task<Message> {
        match event {
            ClaudeLoginEvent::Finished { flow_id, result } => {
                let Some(login) = self.claude_login.as_mut() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                self.claude_login_handle = None;
                match *result {
                    Ok(success) => {
                        login.status = ClaudeLoginStatus::Succeeded;
                        login.error = None;
                        let account_id = success.account.id.clone();
                        let account_label = success.account.label.clone();
                        self.write_config(|new_config| {
                            claude::apply_login_account(new_config, success.account.clone());
                        });
                        runtime::reconcile_provider(
                            &self.config,
                            &mut self.state,
                            ProviderId::Claude,
                        );
                        let mut account = ProviderAccountRuntimeState::empty(
                            ProviderId::Claude,
                            account_id.clone(),
                            account_label,
                        );
                        if let Some(snapshot) = success.snapshot {
                            account.source_label = Some(snapshot.source.clone());
                            account.last_success_at = Some(chrono::Utc::now());
                            account.health = crate::model::ProviderHealth::Ok;
                            account.snapshot = Some(snapshot);
                        }
                        account.auth_state = crate::model::AuthState::Ready;
                        account.error = None;
                        let refresh_succeeded =
                            account.health == ProviderHealth::Ok && account.snapshot.is_some();
                        self.state.upsert_account(account);
                        if let Some(provider) = self.state.provider_mut(ProviderId::Claude) {
                            if !provider.selected_account_ids.contains(&account_id) {
                                provider.selected_account_ids.push(account_id);
                            }
                            provider.account_status = AccountSelectionStatus::Ready;
                            provider.error = None;
                            if refresh_succeeded {
                                provider.legacy_display_snapshot = None;
                            }
                        }
                        runtime::persist_state(&self.state);
                        refresh_provider_task(&self.config, &mut self.state, ProviderId::Claude)
                    }
                    Err(error) => {
                        login.status = ClaudeLoginStatus::Failed;
                        login.error = Some(error);
                        Task::none()
                    }
                }
            }
        }
    }

    pub(super) fn reauthenticate_cursor_account(&mut self, account_id: &str) -> Task<Message> {
        if cursor::find_managed_account(&self.config.cursor_managed_accounts, account_id).is_none()
        {
            return Task::none();
        }
        self.start_cursor_scan()
    }

    pub(super) fn start_cursor_scan(&mut self) -> Task<Message> {
        if matches!(self.cursor_scan, CursorScanState::Scanning) {
            return Task::none();
        }
        self.cursor_scan = CursorScanState::Scanning;
        self.cursor_scan_result = None;
        let existing = self.config.cursor_managed_accounts.clone();
        Task::perform(
            async move {
                let client = runtime::http_client();
                cursor::scan(&client, &existing).await
            },
            |(state, result)| cosmic::Action::App(Message::CursorScanComplete(state, result)),
        )
    }

    pub(super) fn handle_cursor_scan_complete(
        &mut self,
        state: CursorScanState,
        result: Option<CursorScanResult>,
    ) {
        self.cursor_scan = state;
        self.cursor_scan_result = result;
    }

    pub(super) fn confirm_cursor_scan(&mut self) -> Task<Message> {
        let Some(result) = self.cursor_scan_result.take() else {
            self.cursor_scan = CursorScanState::Idle;
            return Task::none();
        };
        match cursor::confirm_scan(&self.config.cursor_managed_accounts, &result) {
            Ok(new_account) => {
                let mut applied_account = new_account.clone();
                self.write_config(|new_config| {
                    applied_account = cursor::upsert_managed_account(new_config, new_account);
                });
                runtime::reconcile_provider(&self.config, &mut self.state, ProviderId::Cursor);
                let account_id = cursor::managed_account_id(&applied_account.id);
                let account_label = applied_account.email.clone();
                let mut account = ProviderAccountRuntimeState::empty(
                    ProviderId::Cursor,
                    account_id.clone(),
                    account_label,
                );
                account.auth_state = crate::model::AuthState::Ready;
                account.error = None;
                self.state.upsert_account(account);
                if let Some(provider) = self.state.provider_mut(ProviderId::Cursor) {
                    if !provider.selected_account_ids.contains(&account_id) {
                        provider.selected_account_ids.push(account_id);
                    }
                    provider.account_status = AccountSelectionStatus::Ready;
                    provider.error = None;
                }
                runtime::persist_state(&self.state);
                self.cursor_scan = CursorScanState::Idle;
                refresh_provider_task(&self.config, &mut self.state, ProviderId::Cursor)
            }
            Err(error) => {
                self.cursor_scan = CursorScanState::Error(error);
                Task::none()
            }
        }
    }

    pub(super) fn dismiss_cursor_scan(&mut self) {
        self.cursor_scan = CursorScanState::Idle;
        self.cursor_scan_result = None;
    }

    pub(super) fn reauthenticate_gemini_account(&mut self, account_id: &str) -> Task<Message> {
        if self
            .config
            .gemini_managed_accounts
            .iter()
            .all(|a| a.id != account_id)
        {
            return Task::none();
        }
        if self
            .gemini_login
            .as_ref()
            .is_some_and(|login| login.status == GeminiLoginStatus::Running)
        {
            return Task::none();
        }
        self.gemini_login = None;
        let (state, task) = match gemini::prepare_for_reauth(self.config.clone(), account_id) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.gemini_login = Some(GeminiLoginState {
                    flow_id: "failed".to_string(),
                    status: GeminiLoginStatus::Failed,
                    login_url: None,
                    output: Vec::new(),
                    error: Some(error),
                });
                return Task::none();
            }
        };
        self.gemini_login = Some(state);
        let task =
            task.map(|event| cosmic::Action::App(Message::GeminiLoginEvent(Box::new(event))));
        let (task, handle) = task.abortable();
        self.gemini_login_handle = Some(handle);
        task
    }

    pub(super) fn start_gemini_login(&mut self) -> Task<Message> {
        if self
            .gemini_login
            .as_ref()
            .is_some_and(|login| login.status == GeminiLoginStatus::Running)
        {
            return Task::none();
        }
        self.gemini_login = None;
        let (state, task) = match gemini::prepare(self.config.clone()) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.gemini_login = Some(GeminiLoginState {
                    flow_id: "failed".to_string(),
                    status: GeminiLoginStatus::Failed,
                    login_url: None,
                    output: Vec::new(),
                    error: Some(error),
                });
                return Task::none();
            }
        };
        self.gemini_login = Some(state);
        let task =
            task.map(|event| cosmic::Action::App(Message::GeminiLoginEvent(Box::new(event))));
        let (task, handle) = task.abortable();
        self.gemini_login_handle = Some(handle);
        task
    }

    pub(super) fn cancel_gemini_login(&mut self) {
        if let Some(handle) = self.gemini_login_handle.take() {
            handle.abort();
        }
        self.gemini_login = None;
    }

    pub(super) fn handle_gemini_login_event(&mut self, event: GeminiLoginEvent) -> Task<Message> {
        match event {
            GeminiLoginEvent::Output {
                flow_id,
                line,
                login_url,
            } => {
                let Some(login) = self.gemini_login.as_mut() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                if let Some(url) = login_url {
                    login.login_url = Some(url);
                }
                login.output.push(line);
                if login.output.len() > 8 {
                    login.output.remove(0);
                }
                Task::none()
            }
            GeminiLoginEvent::Finished { flow_id, result } => {
                let Some(login) = self.gemini_login.as_mut() else {
                    return Task::none();
                };
                if login.flow_id != flow_id {
                    return Task::none();
                }
                self.gemini_login_handle = None;
                match *result {
                    Ok(success) => {
                        login.status = GeminiLoginStatus::Succeeded;
                        login.error = None;
                        let account_id = success.account.id.clone();
                        let account_label = success.account.label.clone();
                        self.write_config(|new_config| {
                            gemini::apply_login_account(new_config, success.account.clone());
                        });
                        runtime::reconcile_provider(
                            &self.config,
                            &mut self.state,
                            ProviderId::Gemini,
                        );
                        let mut account = ProviderAccountRuntimeState::empty(
                            ProviderId::Gemini,
                            account_id.clone(),
                            account_label,
                        );
                        account.auth_state = crate::model::AuthState::Ready;
                        account.error = None;
                        self.state.upsert_account(account);
                        if let Some(provider) = self.state.provider_mut(ProviderId::Gemini) {
                            if !provider.selected_account_ids.contains(&account_id) {
                                provider.selected_account_ids.push(account_id);
                            }
                            provider.account_status = AccountSelectionStatus::Ready;
                            provider.error = None;
                        }
                        runtime::persist_state(&self.state);
                        refresh_provider_task(&self.config, &mut self.state, ProviderId::Gemini)
                    }
                    Err(error) => {
                        login.status = GeminiLoginStatus::Failed;
                        login.error = Some(error);
                        Task::none()
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
struct CodexMetadataUpdate {
    id: String,
    label: Option<String>,
    email: Option<String>,
    provider_account_id: Option<String>,
}

fn codex_managed_metadata_update(
    account: &ProviderAccountRuntimeState,
) -> Option<CodexMetadataUpdate> {
    let snapshot = account.snapshot.as_ref()?;
    Some(CodexMetadataUpdate {
        id: account.account_id.clone(),
        label: snapshot.identity.email.clone(),
        email: snapshot.identity.email.clone(),
        provider_account_id: snapshot.identity.account_id.clone(),
    })
}

fn apply_codex_metadata_update(
    account: &mut ManagedCodexAccountConfig,
    update: &CodexMetadataUpdate,
) {
    if let Some(label) = &update.label
        && account.label == "Codex account"
    {
        account.label.clone_from(label);
    }
    if update.email.is_some() {
        account.email.clone_from(&update.email);
    }
    if update.provider_account_id.is_some() {
        account
            .provider_account_id
            .clone_from(&update.provider_account_id);
    }
    account.updated_at = chrono::Utc::now();
}

#[derive(Clone)]
struct ClaudeMetadataUpdate {
    id: String,
    label: Option<String>,
    email: Option<String>,
    subscription_type: Option<String>,
}

fn claude_managed_metadata_update(
    account: &ProviderAccountRuntimeState,
) -> Option<ClaudeMetadataUpdate> {
    let snapshot = account.snapshot.as_ref()?;
    Some(ClaudeMetadataUpdate {
        id: account.account_id.clone(),
        label: snapshot.identity.email.clone(),
        email: snapshot.identity.email.clone(),
        subscription_type: snapshot.identity.plan.clone(),
    })
}

fn apply_claude_metadata_update(
    account: &mut ManagedClaudeAccountConfig,
    update: &ClaudeMetadataUpdate,
) {
    if let Some(label) = &update.label {
        account.label.clone_from(label);
    }
    if update.email.is_some() {
        account.email.clone_from(&update.email);
    }
    if update.subscription_type.is_some() {
        account
            .subscription_type
            .clone_from(&update.subscription_type);
    }
    account.updated_at = chrono::Utc::now();
}

#[derive(Clone)]
struct CursorMetadataUpdate {
    config_id: String,
    email: String,
    display_name: Option<String>,
    plan: Option<String>,
}

fn cursor_managed_metadata_update(
    account: &ProviderAccountRuntimeState,
) -> Option<CursorMetadataUpdate> {
    let config_id = cursor::managed_config_id(&account.account_id)?;
    let snapshot = account.snapshot.as_ref()?;
    Some(CursorMetadataUpdate {
        config_id: config_id.to_string(),
        email: snapshot
            .identity
            .email
            .as_deref()
            .map_or_else(|| config_id.to_string(), cursor::normalized_email),
        display_name: snapshot.identity.display_name.clone(),
        plan: snapshot.identity.plan.clone(),
    })
}

fn apply_cursor_metadata_update(
    account: &mut ManagedCursorAccountConfig,
    update: &CursorMetadataUpdate,
) {
    account.label.clone_from(&update.email);
    account.email.clone_from(&update.email);
    if update.display_name.is_some() {
        account.display_name.clone_from(&update.display_name);
    }
    if update.plan.is_some() {
        account.plan.clone_from(&update.plan);
    }
    account.updated_at = chrono::Utc::now();
}
