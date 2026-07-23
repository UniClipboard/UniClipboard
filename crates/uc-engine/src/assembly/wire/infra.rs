use super::*;

/// Create SQLite database connection pool
pub(super) fn create_db_pool(db_path: &PathBuf) -> WiringResult<DbPool> {
    if db_path.as_os_str() != ":memory:" {
        if let Some(parent) = db_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| {
                WiringError::DatabaseInit(format!("Failed to create DB directory: {}", e))
            })?;
        }
    }

    let db_url = db_path
        .to_str()
        .ok_or_else(|| WiringError::DatabaseInit("Invalid database path".to_string()))?;

    init_db_pool(db_url)
        .map_err(|e| WiringError::DatabaseInit(format!("Failed to initialize DB: {}", e)))
}
pub(super) fn build_space_access_ports(
    key_material: &Arc<KeyMaterialStore>,
    current_profile: &Arc<dyn uc_core::ports::security::current_profile::CurrentProfilePort>,
    session: &Arc<InMemorySession>,
) -> SpaceAccessPorts {
    let space_access_adapter = Arc::new(uc_infra::security::DefaultSpaceAccessAdapter::new(
        key_material.clone(),
        current_profile.clone(),
        session.clone(),
    ));
    SpaceAccessPorts::from_adapter(space_access_adapter)
}
pub(super) fn build_search_assembly(
    db_pool_for_search: DbPool,
    space_access_ports: &SpaceAccessPorts,
    current_profile: &Arc<dyn uc_core::ports::security::current_profile::CurrentProfilePort>,
) -> SearchAssembly {
    let search_key_derivation: Arc<dyn SearchKeyDerivationPort> =
        Arc::new(HkdfSearchKeyDerivation::new(
            space_access_ports.derive_subkey.clone(),
            current_profile.clone(),
        ));
    // One concrete adapter, coerced into both the index port and the maintenance
    // port (ports.md §8.3: one Arc behind several narrow ports).
    let sqlite_search_index = Arc::new(SqliteSearchIndex::new(
        db_pool_for_search,
        current_profile.clone(),
        search_key_derivation.clone(),
    ));
    let search_index: Arc<dyn SearchIndexPort> = sqlite_search_index.clone();
    let search_maintenance: Arc<dyn SearchIndexMaintenancePort> = sqlite_search_index;
    let search_pipeline = Arc::new(SearchPipeline::new());
    SearchAssembly {
        search_index,
        search_maintenance,
        search_key_derivation,
        search_pipeline,
    }
}

/// Encryption decorators + cipher ports. `blob_cipher` is the business AEAD
/// adapter shared by the decorators and the transfer cipher; all share the one
pub(super) fn build_cipher_decorators(
    session: &Arc<InMemorySession>,
    clipboard_event_repo: &Arc<dyn ClipboardEventWriterPort>,
    representation_repo: &Arc<dyn ClipboardRepresentationStore>,
) -> CipherDecorators {
    // BlobCipherPort — business AEAD adapter shared by the decorators.
    let blob_cipher: Arc<dyn uc_core::ports::security::BlobCipherPort> =
        Arc::new(uc_infra::security::BlobCipherAdapter::new(session.clone()));

    // TransferCipherPort — uc-application clipboard_sync encrypts/decrypts V3
    // network bytes through this port, sharing the same InMemorySession.
    let transfer_cipher: Arc<dyn uc_core::ports::security::TransferCipherPort> = Arc::new(
        uc_infra::clipboard::TransferCipherAdapter::new(session.clone()),
    );

    // Wrap ports with encryption decorators.
    let encrypting_event_writer: Arc<dyn ClipboardEventWriterPort> = Arc::new(
        EncryptingClipboardEventWriter::new(clipboard_event_repo.clone(), blob_cipher.clone()),
    );

    // Concrete decorator Arc: coerced into the legacy aggregate port and into
    // each application-facing representation intent port. Reads decrypt;
    // background workers keep the inner store via `infra.representation_repo`.
    let decrypting_rep_repo_concrete = Arc::new(DecryptingClipboardRepresentationRepository::new(
        representation_repo.clone(),
        blob_cipher.clone(),
    ));
    let decrypting_rep_repo: Arc<dyn ClipboardRepresentationStore> =
        decrypting_rep_repo_concrete.clone();
    let representation_ports = ClipboardRepresentationPorts {
        get: decrypting_rep_repo_concrete.clone(),
        get_by_blob_id: decrypting_rep_repo_concrete.clone(),
        list_for_event: decrypting_rep_repo_concrete.clone(),
        update_processing_result: decrypting_rep_repo_concrete,
    };

    CipherDecorators {
        blob_cipher,
        transfer_cipher,
        encrypting_event_writer,
        decrypting_rep_repo,
        representation_ports,
    }
}

/// Background blob-processing components. `representation_cache` /
/// `spool_manager` are concrete (BackgroundRuntimeDeps needs them by-value);
pub(super) fn build_blob_processing_assembly(
    storage_config: &Arc<ClipboardStorageConfig>,
    spool_dir: PathBuf,
) -> WiringResult<BlobProcessingAssembly> {
    let representation_cache = Arc::new(RepresentationCache::new(
        storage_config.cache_max_entries,
        storage_config.cache_max_bytes,
    ));
    let representation_cache_port: Arc<dyn RepresentationCachePort> = representation_cache.clone();

    let spool_manager = Arc::new(
        SpoolManager::new(spool_dir, storage_config.spool_max_bytes)
            .map_err(|e| WiringError::BlobStorageInit(format!("Failed to create spool: {}", e)))?,
    );

    let (worker_tx, worker_rx) = mpsc::channel::<RepresentationId>(100);

    // DurableSpoolQueue writes bytes to disk synchronously before returning,
    // ensuring spool files survive process exits.
    let spool_queue: Arc<dyn SpoolQueuePort> = Arc::new(DurableSpoolQueue::new(
        spool_manager.clone(),
        worker_tx.clone(),
    ));

    let clipboard_change_origin = new_in_memory_change_origin();

    // Payload resolver for resolving staged/processing payloads.
    let payload_resolver: Arc<dyn ClipboardPayloadResolverPort> =
        Arc::new(ClipboardPayloadResolver::new(
            representation_cache.clone(),
            spool_manager.clone(),
            worker_tx.clone(),
        ));

    Ok(BlobProcessingAssembly {
        representation_cache,
        representation_cache_port,
        spool_manager,
        spool_queue,
        payload_resolver,
        worker_tx,
        worker_rx,
        clipboard_change_origin,
    })
}

/// Build the whole-installation config-migration facade (export / import preview
/// / staged import). Assembled in the sync wiring context because its inputs
/// (secure storage, db pool, local identity, filesystem layout, profile) are not
/// reconstructable from the abstract `AppDeps` ports; the composed facade travels
/// on `AppDeps.config_migration`.
///
/// The local-identity port reads the device fingerprint for the export manifest
/// from the same secure-storage view used by the running node. Single-user mode
pub(super) fn build_config_migration_facade(
    secure_storage: &Arc<dyn SecureStoragePort>,
    db_pool_for_config_migration: DbPool,
    clock: &Arc<dyn ClockPort>,
    setup_status: &Arc<dyn SetupStatusPort>,
    space_access_ports: &SpaceAccessPorts,
    app_version: String,
    source_mode: ConfigSourceMode,
    migration_paths: ConfigMigrationPaths,
) -> Arc<ConfigMigrationFacade> {
    let config_migration_profile = ProfileId::from("default");
    let config_migration_local_identity: Arc<dyn LocalIdentityPort> =
        Arc::new(IrohIdentityStore::new(
            secure_storage.clone(),
            Arc::new(Sha256IdentityFingerprintFactory),
        ));
    let config_migration_adapter = Arc::new(
        ConfigMigrationAdapter::new(
            secure_storage.clone(),
            db_pool_for_config_migration,
            config_migration_local_identity,
            clock.clone(),
            migration_paths,
            config_migration_profile,
            source_mode,
        )
        .with_app_version(app_version),
    );
    Arc::new(ConfigMigrationFacade::new(ConfigMigrationDeps {
        export_bundle: config_migration_adapter.clone(),
        preview_import: config_migration_adapter.clone(),
        stage_import: config_migration_adapter.clone(),
        setup_status: setup_status.clone(),
        is_unlocked: space_access_ports.is_unlocked.clone(),
    }))
}

pub(super) fn create_infra_layer(
    db_pool: DbPool,
    vault_path: &PathBuf,
    settings_path: &PathBuf,
    app_data_root: &PathBuf,
    secure_storage: Arc<dyn SecureStoragePort>,
) -> WiringResult<InfraLayer> {
    let db_executor = Arc::new(DieselSqliteExecutor::new(db_pool));

    let entry_row_mapper = ClipboardEntryRowMapper;
    let selection_row_mapper = ClipboardSelectionRowMapper;
    let blob_row_mapper = BlobRowMapper;
    let _representation_row_mapper = RepresentationRowMapper;

    let entry_repo = DieselClipboardEntryRepository::new(
        Arc::clone(&db_executor),
        entry_row_mapper,
        selection_row_mapper,
        ClipboardEntryRowMapper, // ZST - can instantiate again
    );
    // Keep a concrete Arc so it can be coerced into each narrow entry intent
    // port. The entry adapter still implements the aggregate ClipboardEntryStore
    // (the intent-port impls delegate to it), but no consumer needs the wide
    // trait object, so it is not exposed through the ports bundle.
    let entry_repo_arc = Arc::new(entry_repo);
    // Availability (DB reps + filesystem) and transactional entry-replace are
    // separate adapters over the same executor; the inbound upgrade path uses
    // them to turn a partial entry into a complete one in place.
    let entry_availability_repo: Arc<dyn uc_core::ports::clipboard::CheckEntryAvailabilityPort> =
        Arc::new(DieselEntryAvailabilityRepository::new(Arc::clone(
            &db_executor,
        )));
    let entry_replace_repo: Arc<dyn uc_core::ports::clipboard::ReplaceEntryContentPort> = Arc::new(
        DieselClipboardEntryReplaceRepository::new(Arc::clone(&db_executor)),
    );
    let clipboard_entry_ports = ClipboardEntryPorts {
        get: entry_repo_arc.clone(),
        list: entry_repo_arc.clone(),
        save: entry_repo_arc.clone(),
        touch: entry_repo_arc.clone(),
        set_favorite: entry_repo_arc.clone(),
        delete: entry_repo_arc.clone(),
        delete_with_receive_state: entry_repo_arc.clone(),
        find_by_snapshot_hash: entry_repo_arc.clone(),
        get_snapshot_hash: entry_repo_arc,
        availability: entry_availability_repo,
        replace_content: entry_replace_repo,
    };

    let event_row_mapper = ClipboardEventRowMapper;
    let clipboard_event_repo_impl = Arc::new(DieselClipboardEventRepository::new(
        Arc::clone(&db_executor),
        event_row_mapper,
        RepresentationRowMapper,
    ));
    // 同一份 impl 同时满足"写"和"读"两个端口契约,unsize 两次拿到两个 Arc。
    let clipboard_event_repo: Arc<dyn ClipboardEventWriterPort> =
        Arc::clone(&clipboard_event_repo_impl) as Arc<_>;
    let clipboard_event_reader_repo: Arc<dyn uc_core::ports::ClipboardEventRepositoryPort> =
        clipboard_event_repo_impl as Arc<_>;

    let rep_repo = DieselClipboardRepresentationRepository::new(Arc::clone(&db_executor));
    let representation_repo: Arc<dyn ClipboardRepresentationStore> = Arc::new(rep_repo);

    let entry_delivery_repo: Arc<dyn uc_core::ports::EntryDeliveryRepositoryPort> = Arc::new(
        uc_infra::db::repositories::DieselEntryDeliveryRepository::new(Arc::clone(&db_executor)),
    );

    // NOTE: the entry-file-set repo seals its path columns with a per-session
    // subkey, which needs `current_profile` + space access — both wired after
    // `platform`. It is therefore constructed at the orchestrator level once
    // those exist (mirroring the search index), over `infra.db_executor`.

    let member_repo_impl =
        DieselSpaceMemberRepository::new(Arc::clone(&db_executor), SpaceMemberRowMapper);
    let member_repo: Arc<dyn uc_core::MemberRepositoryPort> = Arc::new(member_repo_impl);

    let trusted_peer_repo_impl =
        DieselTrustedPeerRepository::new(Arc::clone(&db_executor), TrustedPeerRowMapper);
    let trusted_peer_repo: Arc<dyn uc_core::TrustedPeerRepositoryPort> =
        Arc::new(trusted_peer_repo_impl);

    let peer_addr_repo_impl =
        DieselPeerAddressRepository::new(Arc::clone(&db_executor), PeerAddressRowMapper);
    let peer_addr_repo: Arc<dyn uc_core::ports::PeerAddressRepositoryPort> =
        Arc::new(peer_addr_repo_impl);

    let blob_reference_repo: Arc<dyn BlobReferenceRepositoryPort> =
        Arc::new(DieselBlobReferenceRepository::new(Arc::clone(&db_executor)));

    let blob_repo = DieselBlobRepository::new(
        Arc::clone(&db_executor),
        blob_row_mapper,
        BlobRowMapper, // ZST - can instantiate again
    );
    let blob_repository: Arc<dyn BlobRepositoryPort> = Arc::new(blob_repo);

    let thumbnail_repo_impl = DieselThumbnailRepository::new(Arc::clone(&db_executor));
    let thumbnail_repo: Arc<dyn ThumbnailRepositoryPort> = Arc::new(thumbnail_repo_impl);
    let thumbnail_generator =
        InfraThumbnailGenerator::new(128).map_err(|e| WiringError::ThumbnailInit(e.to_string()))?;
    let thumbnail_generator: Arc<dyn ThumbnailGeneratorPort> = Arc::new(thumbnail_generator);

    let secure_storage_for_key_material = Arc::clone(&secure_storage);

    let keyslot_store = JsonKeySlotStore::new(vault_path.clone());
    let keyslot_store: Arc<dyn uc_infra::fs::key_slot_store::KeySlotStore> =
        Arc::new(keyslot_store);

    let key_material = Arc::new(KeyMaterialStore::new(
        secure_storage_for_key_material,
        keyslot_store,
    ));

    let settings_repo: Arc<dyn SettingsPort> = Arc::new(FileSettingsRepository::new(settings_path));

    let setup_status: Arc<dyn SetupStatusPort> =
        Arc::new(FileSetupStatusRepository::with_defaults(vault_path.clone()));

    // 升级游标——独立小文件，落在 app_data_root 顶层（与 vault/keyring/settings.json
    // 同级），不污染 vault/。schema_version=1，写入走 tempfile + rename 原子化。
    let app_version_state: Arc<dyn AppVersionStatePort> = Arc::new(
        FileAppVersionStateRepository::with_defaults(app_data_root.clone()),
    );

    // 首次同步事件去重 flag——独立小文件 first-sync-state.json，与升级游标同级。
    // 三 flag（attempted / succeeded / file_succeeded）合一，schema_version=1，
    // tempfile + rename 原子化；fan-out race 防护由 port impl 的 Mutex 守护。
    let first_sync_state: Arc<dyn FirstSyncStatePort> = Arc::new(
        FileFirstSyncStateRepository::with_defaults(app_data_root.clone()),
    );

    // Switch-space 4 阶段迁移的状态持久化点；与 setup_status 同目录。
    let migration_state: Arc<dyn uc_core::ports::setup::MigrationStatePort> = Arc::new(
        FileMigrationStateRepository::with_defaults(vault_path.clone()),
    );

    // Switch-space backup 表 + 主表 inline_data 批量 IO；常态业务代码不
    // 应触碰，由 SpaceSetupFacade::switch_space 内部使用。
    let blob_migration_repo: Arc<dyn uc_core::ports::clipboard::BlobMigrationRepoPort> =
        Arc::new(DieselBlobMigrationRepository::new(Arc::clone(&db_executor)));

    let clock: Arc<dyn ClockPort> = Arc::new(SystemClock);
    let hash: Arc<dyn ContentHashPort> = Arc::new(Blake3Hasher);

    let selection_repo_impl = DieselClipboardSelectionRepository::new(Arc::clone(&db_executor));
    let selection_repo: Arc<dyn ClipboardSelectionRepositoryPort> = Arc::new(selection_repo_impl);

    // Keep a concrete Arc so it can be coerced into each narrow device-repo
    // intent port. The adapter implements the aggregate MobileDeviceStore and
    // each intent-port impl delegates to it (ports.md §8.3); only the narrow
    // ports are exposed upward.
    #[cfg(feature = "lan-compat")]
    let mobile_device_repo_arc = Arc::new(DieselMobileDeviceRepository::new(
        Arc::clone(&db_executor),
        MobileDeviceRowMapper,
    ));
    #[cfg(feature = "lan-compat")]
    let mobile_device_ports = MobileDevicePorts {
        find_by_username: mobile_device_repo_arc.clone(),
        find_by_id: mobile_device_repo_arc.clone(),
        list: mobile_device_repo_arc.clone(),
        save: mobile_device_repo_arc.clone(),
        delete: mobile_device_repo_arc.clone(),
        update: mobile_device_repo_arc,
    };

    // endpoint_info adapter:进程级单例,daemon LAN listener 与 facade 各持
    // 一份 Arc 共享同一份内存。整个进程只跑一次 `wire_dependencies`,这里
    // new 一份就足够。
    #[cfg(feature = "lan-compat")]
    let mobile_sync_endpoint_info =
        Arc::new(uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter::new());

    let infra = InfraLayer {
        clipboard_entry_ports,
        clipboard_event_repo,
        clipboard_event_reader_repo,
        entry_delivery_repo,
        db_executor,
        representation_repo,
        selection_repo,
        member_repo,
        trusted_peer_repo,
        peer_addr_repo,
        blob_reference_repo,
        migration_state,
        blob_migration_repo,
        blob_repository,
        thumbnail_repo,
        thumbnail_generator,
        key_material,
        settings_repo,
        setup_status,
        app_version_state,
        first_sync_state,
        clock,
        hash,
        #[cfg(feature = "lan-compat")]
        mobile_device_ports,
        #[cfg(feature = "lan-compat")]
        mobile_sync_endpoint_info,
    };

    Ok(infra)
}
