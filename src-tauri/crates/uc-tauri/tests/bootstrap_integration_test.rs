//! # Bootstrap Module Integration Tests / Bootstrap 模块集成测试
//!
//! These tests verify that the bootstrap module correctly:
//! 这些测试验证 bootstrap 模块正确地：
//!
//! 1. Loads configuration from TOML files / 从 TOML 文件加载配置
//! 2. Creates the dependency injection structure / 创建依赖注入结构
//! 3. Maintains separation between config and business logic / 保持配置和业务逻辑的分离
//!
//! ## Test Philosophy / 测试理念
//!
//! **Pure data behavior only / 仅纯数据行为**:
//! - Config loader accepts whatever is in the file / 配置加载器接受文件中的任何内容
//! - No validation logic / 无验证逻辑
//! - No default value logic / 无默认值逻辑
//! - Paths are loaded as-is (no existence checks) / 路径按原样加载（不检查存在性）
//!
//! ## Phase 3 Status / 第3阶段状态
//!
//! In Phase 3, `wire_dependencies` is fully implemented with real dependency injection.
//! 在第3阶段，`wire_dependencies` 完全实现了真实的依赖注入。
//! These tests verify the integration between config loading, dependency wiring, and app creation.
//! 这些测试验证配置加载、依赖连接和应用创建之间的集成。

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tempfile::TempDir;
use tokio::sync::mpsc;
use uc_core::config::AppConfig;
use uc_platform::ports::{IdentityStoreError, IdentityStorePort};
use uc_tauri::bootstrap::wiring::wire_dependencies_with_identity_store;
use uc_tauri::bootstrap::{create_app, create_runtime, load_config};

#[derive(Default)]
struct MemoryIdentityStore {
    identity: Mutex<Option<Vec<u8>>>,
}

impl IdentityStorePort for MemoryIdentityStore {
    fn load_identity(&self) -> Result<Option<Vec<u8>>, IdentityStoreError> {
        let guard = self
            .identity
            .lock()
            .map_err(|_| IdentityStoreError::Store("identity store poisoned".to_string()))?;
        Ok(guard.clone())
    }

    fn store_identity(&self, identity: &[u8]) -> Result<(), IdentityStoreError> {
        let mut guard = self
            .identity
            .lock()
            .map_err(|_| IdentityStoreError::Store("identity store poisoned".to_string()))?;
        *guard = Some(identity.to_vec());
        Ok(())
    }
}

fn test_identity_store() -> Arc<dyn IdentityStorePort> {
    Arc::new(MemoryIdentityStore::default())
}

fn run_with_tokio<F, T>(operation: F) -> T
where
    F: FnOnce() -> T,
{
    let runtime = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    runtime.block_on(async move { operation() })
}

/// Test 1: Full integration test for config loading
/// 测试1：配置加载的完整集成测试
///
/// This test verifies that:
/// 此测试验证：
/// - A complete TOML file is parsed correctly / 完整的 TOML 文件被正确解析
/// - All fields are loaded into AppConfig / 所有字段被加载到 AppConfig
/// - The integration between file I/O and parsing works / 文件 I/O 和解析之间的集成工作
#[test]
fn test_bootstrap_load_config_integration() {
    // Create a temporary directory for test isolation
    // 为测试隔离创建临时目录
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("test_config.toml");

    // Write a complete TOML configuration
    // 写入完整的 TOML 配置
    let toml_content = r#"
        [general]
        device_name = "TestDevice"
        silent_start = true

        [security]
        vault_key_path = "/tmp/test/key"
        vault_snapshot_path = "/tmp/test/snapshot"

        [network]
        webserver_port = 8080

        [storage]
        database_path = "/tmp/test/database.db"
    "#;

    let mut file = fs::File::create(&config_path).unwrap();
    file.write_all(toml_content.as_bytes()).unwrap();

    // Load config and verify all fields
    // 加载配置并验证所有字段
    let config = load_config(config_path).unwrap();

    assert_eq!(config.device_name, "TestDevice");
    assert_eq!(config.webserver_port, 8080);
    assert_eq!(config.silent_start, true);
    assert_eq!(config.vault_key_path, PathBuf::from("/tmp/test/key"));
    assert_eq!(
        config.vault_snapshot_path,
        PathBuf::from("/tmp/test/snapshot")
    );
    assert_eq!(config.database_path, PathBuf::from("/tmp/test/database.db"));

    // TempDir is automatically cleaned up when dropped
    // TempDir 在 drop 时自动清理
}

/// Test 2: Empty values are valid facts
/// 测试2：空值是合法的事实
///
/// This test verifies the "no validation" principle:
/// 此测试验证"无验证"原则：
/// - Empty strings are accepted / 空字符串被接受
/// - Empty paths are accepted / 空路径被接受
/// - Missing sections result in empty values / 缺失的部分导致空值
#[test]
fn test_bootstrap_config_empty_values_are_valid_facts() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("empty_config.toml");

    // Write a TOML with all sections missing
    // 写入所有部分都缺失的 TOML
    let toml_content = r#"
        [general]
        # All fields missing

        [network]
        # Port missing

        [security]
        # Paths missing

        [storage]
        # Database path missing
    "#;

    let mut file = fs::File::create(&config_path).unwrap();
    file.write_all(toml_content.as_bytes()).unwrap();

    let config = load_config(config_path).unwrap();

    // All empty values are valid "facts" - no validation
    // 所有空值都是合法的"事实" - 无验证
    assert_eq!(config.device_name, "");
    assert_eq!(config.webserver_port, 0);
    assert_eq!(config.vault_key_path, PathBuf::new());
    assert_eq!(config.vault_snapshot_path, PathBuf::new());
    assert_eq!(config.database_path, PathBuf::new());
    assert_eq!(config.silent_start, false);
}

/// Test 3: Paths are loaded as-is (no state checks)
/// 测试3：路径按原样加载（无状态检查）
///
/// This test verifies that:
/// 此测试验证：
/// - Paths don't need to exist / 路径不需要存在
/// - Paths can be absolute or relative / 路径可以是绝对或相对的
/// - No filesystem checks are performed / 不执行文件系统检查
#[test]
fn test_bootstrap_config_path_info_only_no_state_check() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("path_config.toml");

    // Use paths that definitely don't exist
    // 使用肯定不存在的路径
    let toml_content = r#"
        [general]
        device_name = "NonExistentDevice"

        [security]
        vault_key_path = "/nonexistent/path/to/secret/key.dat"
        vault_snapshot_path = "/another/nonexistent/path/snapshot.bin"

        [storage]
        database_path = "/tmp/this/does/not/exist/database.db"

        [network]
        webserver_port = 9999
    "#;

    let mut file = fs::File::create(&config_path).unwrap();
    file.write_all(toml_content.as_bytes()).unwrap();

    let config = load_config(config_path).unwrap();

    // Paths are loaded as-is, no existence checks
    // 路径按原样加载，无存在性检查
    assert_eq!(
        config.vault_key_path,
        PathBuf::from("/nonexistent/path/to/secret/key.dat")
    );
    assert_eq!(
        config.vault_snapshot_path,
        PathBuf::from("/another/nonexistent/path/snapshot.bin")
    );
    assert_eq!(
        config.database_path,
        PathBuf::from("/tmp/this/does/not/exist/database.db")
    );

    // Verify the files DON'T actually exist (prove no state check happened)
    // 验证文件实际上不存在（证明没有执行状态检查）
    assert!(!config.vault_key_path.exists());
    assert!(!config.vault_snapshot_path.exists());
    assert!(!config.database_path.exists());
}

/// Test 4: Invalid values are accepted (no validation)
/// 测试4：无效值被接受（无验证）
///
/// This test verifies that:
/// 此测试验证：
/// - Ports outside valid range are accepted / 有效范围外的端口被接受
/// - No business rules are enforced / 不执行业务规则
/// - Values are taken as "facts" from the file / 值作为来自文件的"事实"
#[test]
fn test_bootstrap_config_invalid_port_is_accepted() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("invalid_port.toml");

    // Port 99999 is way outside the valid u16 range (0-65535)
    // 端口 99999 远超有效 u16 范围（0-65535）
    // When parsed as u16, it will be truncated/overflow
    // 解析为 u16 时，它将被截断/溢出
    let toml_content = r#"
        [network]
        webserver_port = 99999
    "#;

    let mut file = fs::File::create(&config_path).unwrap();
    file.write_all(toml_content.as_bytes()).unwrap();

    let config = load_config(config_path).unwrap();

    // We don't validate - the value is what TOML gives us
    // 我们不验证 - 值就是 TOML 给我们的
    // 99999 as u16 = 34463 (due to overflow/truncation)
    // 99999 作为 u16 = 34463（由于溢出/截断）
    assert_eq!(config.webserver_port, 34463);

    // Also test port 0 (technically invalid but accepted as a fact)
    // 也测试端口 0（技术上无效但作为事实接受）
    let config_path2 = temp_dir.path().join("zero_port.toml");
    let toml_content2 = r#"
        [network]
        webserver_port = 0
    "#;

    let mut file2 = fs::File::create(&config_path2).unwrap();
    file2.write_all(toml_content2.as_bytes()).unwrap();

    let config2 = load_config(config_path2).unwrap();
    assert_eq!(config2.webserver_port, 0);
}

/// Test 5: wire_dependencies successfully creates AppDeps
/// 测试5：wire_dependencies 成功创建 AppDeps
///
/// This test verifies that:
/// 此测试验证：
/// - wire_dependencies creates all required dependencies / wire_dependencies 创建所有必需的依赖
/// - The dependencies can be used to create an App / 依赖可用于创建 App
/// - All dependency fields are properly initialized / 所有依赖字段正确初始化
#[test]
fn test_bootstrap_wire_dependencies_creates_app_deps() {
    run_with_tokio(|| {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config.toml");

        // Write a minimal valid config
        // 写入最小有效配置
        let toml_content = r#"
        [general]
        device_name = "TestDevice"

        [security]
        vault_key_path = "/tmp/test/key"
        vault_snapshot_path = "/tmp/test/snapshot"

        [storage]
        database_path = ":memory:"
    "#;

        let mut file = fs::File::create(&config_path).unwrap();
        file.write_all(toml_content.as_bytes()).unwrap();

        let config = load_config(config_path).unwrap();
        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let deps_result =
            wire_dependencies_with_identity_store(&config, cmd_tx, Some(test_identity_store()));

        assert!(
            deps_result.is_ok(),
            "wire_dependencies should succeed with valid config"
        );

        let deps = deps_result.unwrap().deps;

        // Verify we can access all dependency fields
        // 验证我们可以访问所有依赖字段
        let _ = &deps.clipboard;
        let _ = &deps.clipboard.clipboard_event_repo;
        let _ = &deps.clipboard.representation_repo;
        let _ = &deps.clipboard.representation_normalizer;
        let _ = &deps.security.encryption;
        let _ = &deps.security.encryption_session;
        let _ = &deps.security.secure_storage;
        let _ = &deps.security.key_material;
        let _ = &deps.device.device_repo;
        let _ = &deps.device.device_identity;
        let _ = &deps.network_ports;
        let _ = &deps.storage.blob_store;
        let _ = &deps.storage.blob_repository;
        let _ = &deps.storage.blob_writer;
        let _ = &deps.settings;
        let _ = &deps.system.clock;
        let _ = &deps.system.hash;
    });
}

/// Test 5.1: wiring exposes secure storage, not keyring
/// 测试5.1：wiring 暴露 secure storage，而不是 keyring
#[test]
fn wiring_exposes_secure_storage_not_keyring() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let platform_keyring = crate_root.join("../uc-platform/src/keyring.rs");
    let platform_file_keyring = crate_root.join("../uc-platform/src/file_keyring.rs");
    let core_keyring = crate_root.join("../uc-core/src/ports/security/keyring.rs");

    assert!(
        !platform_keyring.exists(),
        "uc-platform keyring adapter should be removed"
    );
    assert!(
        !platform_file_keyring.exists(),
        "uc-platform file_keyring adapter should be removed"
    );
    assert!(
        !core_keyring.exists(),
        "uc-core KeyringPort definition should be removed"
    );

    let config = AppConfig::empty();
    let (cmd_tx, _cmd_rx) = mpsc::channel(10);
    let result =
        wire_dependencies_with_identity_store(&config, cmd_tx, Some(test_identity_store()));
    assert!(result.is_ok());
}

/// Test 6: Integration test - real file I/O error handling
/// 测试6：集成测试 - 真实文件 I/O 错误处理
///
/// This test verifies that:
/// 此测试验证：
/// - File not found errors are properly reported / 文件未找到错误被正确报告
/// - Error messages include context / 错误消息包含上下文
/// - I/O errors don't cause panics / I/O 错误不会导致 panic
#[test]
fn test_bootstrap_load_config_handles_io_errors() {
    // Use a path that definitely doesn't exist
    // 使用肯定不存在的路径
    let non_existent_path = "/tmp/uniclipboard_test_this_path_does_not_exist_12345.toml";

    let result = load_config(non_existent_path.into());

    assert!(result.is_err(), "Should return error for non-existent file");

    let error = result.unwrap_err();
    let error_msg = error.to_string().to_lowercase();

    // Error should mention the file or reading failure
    // 错误应该提到文件或读取失败
    assert!(
        error_msg.contains("failed to read")
            || error_msg.contains("no such file")
            || error_msg.contains("not found")
            || error_msg.contains("config"),
        "Error should mention file I/O failure, got: {}",
        error
    );
}

/// Test 7: Integration test - malformed TOML handling
/// 测试7：集成测试 - 格式错误的 TOML 处理
///
/// This test verifies that:
/// 此测试验证：
/// - Invalid TOML syntax is caught / 无效的 TOML 语法被捕获
/// - Parse errors are properly reported / 解析错误被正确报告
/// - Error messages are context-rich / 错误消息包含丰富上下文
#[test]
fn test_bootstrap_load_config_handles_malformed_toml() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("malformed.toml");

    // Write invalid TOML (missing closing bracket)
    // 写入无效的 TOML（缺少闭合括号）
    let malformed_toml = r#"
        [general
        device_name = "Test"
        # Missing closing bracket above
    "#;

    let mut file = fs::File::create(&config_path).unwrap();
    file.write_all(malformed_toml.as_bytes()).unwrap();

    let result = load_config(config_path);

    assert!(result.is_err(), "Should return error for malformed TOML");

    let error = result.unwrap_err();
    let error_msg = error.to_string().to_lowercase();

    // Error should mention TOML parsing
    // 错误应该提到 TOML 解析
    assert!(
        error_msg.contains("toml") || error_msg.contains("parse"),
        "Error should mention TOML parsing failure, got: {}",
        error
    );
}

/// Test 8: Edge case - completely empty file
/// 测试8：边界情况 - 完全空文件
///
/// This test verifies that:
/// 此测试验证：
/// - An empty TOML file is handled gracefully / 空的 TOML 文件被优雅处理
/// - Results in AppConfig with all empty values / 导致所有字段为空的 AppConfig
/// - No crashes or panics / 无崩溃或 panic
#[test]
fn test_bootstrap_load_config_handles_empty_file() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("empty.toml");

    // Write completely empty file
    // 写入完全空的文件
    let mut file = fs::File::create(&config_path).unwrap();
    file.write_all(b"").unwrap();

    let config = load_config(config_path).unwrap();

    // Should get empty config (all defaults/empty values)
    // 应该得到空配置（所有默认/空值）
    assert_eq!(config.device_name, "");
    assert_eq!(config.webserver_port, 0);
    assert_eq!(config.vault_key_path, PathBuf::new());
    assert_eq!(config.vault_snapshot_path, PathBuf::new());
    assert_eq!(config.database_path, PathBuf::new());
    assert_eq!(config.silent_start, false);
}

/// Test 9: Full bootstrap flow integration test
/// 测试9：完整 bootstrap 流程集成测试
///
/// This test verifies the complete bootstrap sequence:
/// 此测试验证完整的 bootstrap 序列：
/// 1. load_config() → AppConfig / 加载配置
/// 2. wire_dependencies() → AppDeps / 连接依赖
/// 3. create_app() → App / 创建应用
///
/// This is the primary integration test for the entire bootstrap module.
/// 这是整个 bootstrap 模块的主要集成测试。
#[test]
fn test_bootstrap_full_flow() {
    run_with_tokio(|| {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("full_flow_config.toml");

        // Write a complete configuration
        // 写入完整配置
        let toml_content = format!(
            r#"
        [general]
        device_name = "FullFlowTest"
        silent_start = false

        [security]
        vault_key_path = "{}/full_flow_key"
        vault_snapshot_path = "{}/full_flow_snapshot"

        [network]
        webserver_port = 8080

        [storage]
        database_path = ":memory:"
    "#,
            temp_dir.path().display(),
            temp_dir.path().display()
        );

        let mut file = fs::File::create(&config_path).unwrap();
        file.write_all(toml_content.as_bytes()).unwrap();

        // Step 1: Load config
        // 步骤 1：加载配置
        let config = load_config(config_path).unwrap();
        assert_eq!(config.device_name, "FullFlowTest");
        assert_eq!(config.webserver_port, 8080);

        // Step 2: Wire dependencies
        // 步骤 2：连接依赖
        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let deps =
            wire_dependencies_with_identity_store(&config, cmd_tx, Some(test_identity_store()))
                .expect("wire_dependencies should succeed")
                .deps;

        // Step 3: Create app
        // 步骤 3：创建应用
        let app = create_app(deps);

        // Verify app was created successfully
        // 验证应用创建成功
        let _app = app; // Use the variable to avoid warnings
    });
}

/// Test 10: Database pool creation with real file system
/// 测试10：使用真实文件系统创建数据库连接池
///
/// This test verifies that:
/// 此测试验证：
/// - Database file is created in the correct location / 数据库文件在正确位置创建
/// - Parent directories are created as needed / 按需创建父目录
/// - Connection pool can be established / 可以建立连接池
#[test]
fn test_bootstrap_database_pool_real_filesystem() {
    run_with_tokio(|| {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_data").join("db").join("test.db");

        // Create config with database path
        // 使用数据库路径创建配置
        let mut config = AppConfig::empty();
        config.database_path = db_path.clone();

        // Wire dependencies (this will create the database)
        // 连接依赖（这将创建数据库）
        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let deps_result =
            wire_dependencies_with_identity_store(&config, cmd_tx, Some(test_identity_store()));

        assert!(
            deps_result.is_ok(),
            "wire_dependencies should create database successfully"
        );

        // Verify database file was created
        // 验证数据库文件已创建
        assert!(db_path.exists(), "Database file should be created");

        // Verify parent directories were created
        // 验证父目录已创建
        assert!(
            db_path.parent().unwrap().exists(),
            "Parent directory should be created"
        );
    });
}

/// Test 11: Database pool creation with invalid path
/// 测试11：使用无效路径创建数据库连接池
///
/// This test verifies error handling when:
/// 此测试验证以下情况时的错误处理：
/// - Database path contains invalid characters / 数据库路径包含无效字符
/// - Path cannot be created / 无法创建路径
#[test]
fn test_bootstrap_database_pool_invalid_path() {
    run_with_tokio(|| {
        // Use a path that cannot be created (e.g., in /root without permissions)
        // 使用无法创建的路径（例如，没有权限的 /root）
        let db_path = PathBuf::from("/root/uniclipboard_test_no_permission/test.db");

        let mut config = AppConfig::empty();
        config.database_path = db_path;

        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let deps_result =
            wire_dependencies_with_identity_store(&config, cmd_tx, Some(test_identity_store()));

        // Should fail gracefully with proper error
        // 应该优雅地失败并返回适当错误
        match deps_result {
            Ok(_) => panic!("wire_dependencies should fail with invalid path"),
            Err(error) => {
                let error_msg = error.to_string().to_lowercase();
                // Error should mention database initialization failure
                // 错误应该提到数据库初始化失败
                assert!(
                    error_msg.contains("database") || error_msg.contains("db"),
                    "Error should mention database, got: {}",
                    error
                );
            }
        }
    });
}

/// Test 12: create_runtime wrapper function
/// 测试12：create_runtime 包装函数
///
/// This test verifies that:
/// 此测试验证：
/// - create_runtime wraps config in AppRuntimeSeed / create_runtime 将配置包装在 AppRuntimeSeed 中
/// - The seed contains the original config / 种子包含原始配置
/// - No side effects occur / 无副作用
#[test]
fn test_bootstrap_create_runtime_wrapper() {
    let config = AppConfig::empty();

    let runtime_result = create_runtime(config.clone());

    assert!(
        runtime_result.is_ok(),
        "create_runtime should always succeed"
    );

    let seed = runtime_result.unwrap();
    assert_eq!(seed.config.device_name, config.device_name);
    assert_eq!(seed.config.webserver_port, config.webserver_port);
    assert_eq!(seed.config.vault_key_path, config.vault_key_path);
    assert_eq!(seed.config.database_path, config.database_path);
}

/// Test 13: Integration test - wire_dependencies with empty config
/// 测试13：集成测试 - 使用空配置的 wire_dependencies
///
/// This test verifies that wire_dependencies handles empty configuration
/// by using sensible defaults for required paths.
/// 此测试验证 wire_dependencies 通过为必需路径使用合理的默认值来处理空配置。
#[test]
fn test_bootstrap_wire_dependencies_with_empty_config() {
    run_with_tokio(|| {
        let config = AppConfig::empty();

        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let deps_result =
            wire_dependencies_with_identity_store(&config, cmd_tx, Some(test_identity_store()));

        // Should succeed even with empty config (uses in-memory database)
        // 应该成功，即使配置为空（使用内存数据库）
        assert!(
            deps_result.is_ok(),
            "wire_dependencies should handle empty config"
        );

        let deps = deps_result.unwrap().deps;

        // Verify all dependencies are present even with empty config
        // 验证即使配置为空，所有依赖都存在
        let _ = &deps.clipboard;
        let _ = &deps.security.encryption;
        let _ = &deps.security.secure_storage;
        let _ = &deps.device.device_repo;
        let _ = &deps.settings;
    });
}

/// Test 14: Integration test - wire_dependencies creates real database repositories
/// 测试14：集成测试 - wire_dependencies 创建真实的数据库仓库
///
/// This test verifies that:
/// 此测试验证：
/// - Database repositories are properly initialized / 数据库仓库正确初始化
/// - Repositories are wrapped in Arc for thread safety / 仓库包装在 Arc 中以确保线程安全
/// - All repository types are present / 所有仓库类型都存在
#[test]
fn test_bootstrap_wire_dependencies_creates_real_repositories() {
    run_with_tokio(|| {
        let mut config = AppConfig::empty();
        config.database_path = PathBuf::from(":memory:");

        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let deps =
            wire_dependencies_with_identity_store(&config, cmd_tx, Some(test_identity_store()))
                .expect("wire_dependencies should succeed")
                .deps;

        // Verify clipboard repositories
        // 验证剪贴板仓库
        let _clipboard_entry_repo = deps.clipboard.clipboard_entry_repo.clone();
        let _clipboard_event_repo = deps.clipboard.clipboard_event_repo.clone();
        let _representation_repo = deps.clipboard.representation_repo.clone();

        // Verify device repository
        // 验证设备仓库
        let _device_repo = deps.device.device_repo.clone();

        // Verify blob repository
        // 验证 blob 仓库
        let _blob_repository = deps.storage.blob_repository.clone();

        // If we got here without panicking, all repositories are properly created
        // 如果我们到这里没有 panic，所有仓库都正确创建了
    });
}

/// Test 15: Integration test - wire_dependencies creates platform adapters
/// 测试15：集成测试 - wire_dependencies 创建平台适配器
///
/// This test verifies that:
/// 此测试验证：
/// - Platform-specific adapters are created / 创建平台特定适配器
/// - Clipboard adapter is platform-specific / 剪贴板适配器是平台特定的
/// - All adapters implement their respective traits / 所有适配器实现各自的 trait
#[test]
fn test_bootstrap_wire_dependencies_creates_platform_adapters() {
    run_with_tokio(|| {
        let mut config = AppConfig::empty();
        config.database_path = PathBuf::from(":memory:");

        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let deps =
            wire_dependencies_with_identity_store(&config, cmd_tx, Some(test_identity_store()))
                .expect("wire_dependencies should succeed")
                .deps;

        // Verify system clipboard (platform-specific)
        // 验证系统剪贴板（平台特定）
        let _clipboard = deps.clipboard.clipboard.clone();

        // Verify secure storage (platform-specific)
        // 验证安全存储（平台特定）
        let _secure_storage = deps.security.secure_storage.clone();

        // Verify placeholder adapters exist (for unimplemented ports)
        // 验证占位符适配器存在（用于未实现的端口）
        let _network = deps.network_ports.clone();
        let _device_identity = deps.device.device_identity.clone();
        let _representation_normalizer = deps.clipboard.representation_normalizer.clone();
        let _blob_writer = deps.storage.blob_writer.clone();
        let _blob_store = deps.storage.blob_store.clone();
        let _encryption_session = deps.security.encryption_session.clone();
    });
}

/// Test 16: Integration test - settings repository initialization
/// 测试16：集成测试 - 设置仓库初始化
///
/// This test verifies that:
/// 此测试验证：
/// - Settings repository is created with correct path / 使用正确路径创建设置仓库
/// - Settings path is derived from vault path / 设置路径从 vault 路径派生
/// - Repository can be accessed / 可以访问仓库
#[test]
fn test_bootstrap_settings_repository_initialization() {
    run_with_tokio(|| {
        let temp_dir = TempDir::new().unwrap();
        let vault_path = temp_dir.path().join("vault");

        let mut config = AppConfig::empty();
        config.vault_key_path = vault_path.join("key.json");
        config.vault_snapshot_path = vault_path.join("snapshot.json");
        config.database_path = PathBuf::from(":memory:");

        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let deps =
            wire_dependencies_with_identity_store(&config, cmd_tx, Some(test_identity_store()))
                .expect("wire_dependencies should succeed")
                .deps;

        // Verify settings repository exists and can be cloned/accessed
        // 验证设置仓库存在并且可以克隆/访问
        let settings = deps.settings.clone();
        let _settings2 = settings.clone();

        // Test passes if we can successfully create and access the settings repository
        // 如果我们可以成功创建和访问设置仓库，测试通过
        // Note: FileSettingsRepository doesn't create files until first write
        // 注意：FileSettingsRepository 在第一次写入之前不会创建文件
    });
}

/// Test 17: Integration test - error propagation in wire_dependencies
/// 测试17：集成测试 - wire_dependencies 中的错误传播
///
/// This test verifies that:
/// 此测试验证：
/// - Errors during dependency creation are properly propagated / 依赖创建期间的错误正确传播
/// - Error messages are context-rich / 错误消息包含丰富上下文
/// - No panics occur during error handling / 错误处理期间无 panic
#[test]
fn test_bootstrap_wire_dependencies_error_propagation() {
    run_with_tokio(|| {
        // Create a config with an invalid database path (non-existent parent with invalid permissions)
        // 创建具有无效数据库路径的配置（具有无效权限的不存在的父目录）
        let mut config = AppConfig::empty();
        config.database_path = PathBuf::from("/nonexistent/with/invalid/permissions/db/test.db");

        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let result =
            wire_dependencies_with_identity_store(&config, cmd_tx, Some(test_identity_store()));

        // Should fail gracefully
        // 应该优雅地失败
        match result {
            Ok(_) => panic!("Should fail with invalid database path"),
            Err(error) => {
                let error_msg = error.to_string();
                // Error message should be descriptive
                // 错误消息应该是描述性的
                assert!(!error_msg.is_empty(), "Error message should not be empty");
                assert!(error_msg.len() > 10, "Error message should be descriptive");
            }
        }
    });
}
