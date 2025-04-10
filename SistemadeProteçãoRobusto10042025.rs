use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use sha2::{Sha256, Digest};
use uuid::Uuid;
use serde::{Serialize, Deserialize};
use lazy_static::lazy_static;

// Tipos básicos
type FileID = String;
type FileHash = String;
type Timestamp = u64;

// Estado global do sistema
lazy_static! {
    static ref FILE_REGISTRY: Arc<Mutex<FileRegistry>> = Arc::new(Mutex::new(FileRegistry::new()));
    static ref ID_GENERATOR: Arc<Mutex<UniqueIDGenerator>> = Arc::new(Mutex::new(UniqueIDGenerator::new()));
}

/// Representa o estado de um arquivo monitorado
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileState {
    Clean,
    Modified(SystemTime),
    Infected(SystemTime),
    Quarantined(SystemTime),
    Whitelisted, // Para arquivos do sistema
}

/// Metadados completos de um arquivo monitorado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredFile {
    pub id: FileID,
    pub path: PathBuf,
    pub original_hash: FileHash,
    pub current_hash: FileHash,
    pub state: FileState,
    pub creation_time: SystemTime,
    pub last_scan: SystemTime,
    pub protection_level: u8,
    pub is_system_file: bool,
}

/// Gerenciador de arquivos monitorados
#[derive(Debug, Serialize, Deserialize)]
pub struct FileRegistry {
    files: HashMap<FileID, MonitoredFile>,
    path_to_id: HashMap<PathBuf, FileID>,
    system_hashes: HashSet<FileHash>,
}

/// Gerador de IDs únicos no formato e-b001.000.000.000 até z-b999.999.999.999
#[derive(Debug)]
pub struct UniqueIDGenerator {
    current_prefix: char,
    current_block: [u32; 4],
}

impl UniqueIDGenerator {
    pub fn new() -> Self {
        UniqueIDGenerator {
            current_prefix: 'e',
            current_block: [1, 0, 0, 0],
        }
    }

    pub fn generate(&mut self) -> FileID {
        let id = format!(
            "{}-b{:03}.{:03}.{:03}.{:03}",
            self.current_prefix,
            self.current_block[0],
            self.current_block[1],
            self.current_block[2],
            self.current_block[3]
        );

        // Incrementa o contador
        self.increment_counter();

        id
    }

    fn increment_counter(&mut self) {
        for i in (0..4).rev() {
            self.current_block[i] += 1;
            if self.current_block[i] > 999 {
                self.current_block[i] = 0;
                if i == 0 {
                    self.current_prefix = ((self.current_prefix as u8) + 1) as char;
                    if self.current_prefix > 'z' {
                        self.current_prefix = 'e';
                    }
                }
            } else {
                break;
            }
        }
    }
}

impl FileRegistry {
    pub fn new() -> Self {
        FileRegistry {
            files: HashMap::new(),
            path_to_id: HashMap::new(),
            system_hashes: HashSet::new(),
        }
    }

    /// Adiciona um novo arquivo ao monitoramento
    pub fn add_file(&mut self, path: &Path, is_system_file: bool) -> Result<FileID, String> {
        if !path.exists() {
            return Err("Arquivo não existe".to_string());
        }

        let file_id = ID_GENERATOR.lock().unwrap().generate();
        let file_hash = Self::calculate_file_hash(path)?;

        if is_system_file {
            self.system_hashes.insert(file_hash.clone());
        }

        let file = MonitoredFile {
            id: file_id.clone(),
            path: path.to_path_buf(),
            original_hash: file_hash.clone(),
            current_hash: file_hash,
            state: if is_system_file {
                FileState::Whitelisted
            } else {
                FileState::Clean
            },
            creation_time: SystemTime::now(),
            last_scan: SystemTime::now(),
            protection_level: if is_system_file { 10 } else { 5 },
            is_system_file,
        };

        self.path_to_id.insert(path.to_path_buf(), file_id.clone());
        self.files.insert(file_id.clone(), file);

        Ok(file_id)
    }

    /// Atualiza o estado de um arquivo
    pub fn update_file_state(&mut self, file_id: &FileID, new_state: FileState) -> Result<(), String> {
        if let Some(file) = self.files.get_mut(file_id) {
            file.state = new_state;
            file.last_scan = SystemTime::now();
            Ok(())
        } else {
            Err("ID de arquivo não encontrado".to_string())
        }
    }

    /// Verifica se um hash pertence a um arquivo do sistema
    pub fn is_system_hash(&self, hash: &str) -> bool {
        self.system_hashes.contains(hash)
    }

    /// Calcula o hash SHA-256 de um arquivo
    pub fn calculate_file_hash(path: &Path) -> Result<String, String> {
        let mut file = std::fs::File::open(path)
            .map_err(|e| format!("Falha ao abrir arquivo: {}", e))?;
        
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)
            .map_err(|e| format!("Falha ao ler arquivo: {}", e))?;
        
        let hash = hasher.finalize();
        Ok(format!("{:x}", hash))
    }

    /// Verifica alterações no arquivo
    pub fn check_for_changes(&mut self, file_id: &FileID) -> Result<bool, String> {
        let file = self.files.get(file_id)
            .ok_or("ID de arquivo não encontrado")?;
        
        let current_hash = Self::calculate_file_hash(&file.path)?;
        
        if current_hash != file.current_hash {
            let mut file = self.files.get_mut(file_id).unwrap();
            file.current_hash = current_hash;
            
            // Se for arquivo do sistema e o hash não estiver na whitelist
            if file.is_system_file && !self.system_hashes.contains(&file.current_hash) {
                file.state = FileState::Modified(SystemTime::now());
                Ok(true)
            } 
            // Se não for arquivo do sistema
            else if !file.is_system_file {
                file.state = FileState::Modified(SystemTime::now());
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    /// Obtém todos os arquivos em um determinado estado
    pub fn get_files_by_state(&self, state: FileState) -> Vec<MonitoredFile> {
        self.files.values()
            .filter(|f| f.state == state)
            .cloned()
            .collect()
    }

    /// Recupera um arquivo pelo caminho
    pub fn get_file_by_path(&self, path: &Path) -> Option<MonitoredFile> {
        self.path_to_id.get(path)
            .and_then(|id| self.files.get(id))
            .cloned()
    }
}

/// Sistema de transição de estados
pub struct StateTransitionSystem;

impl StateTransitionSystem {
    pub fn handle_file_change(registry: &mut FileRegistry, file_id: &FileID) -> Result<(), String> {
        let file = registry.files.get(file_id)
            .ok_or("ID de arquivo não encontrado")?;
        
        match &file.state {
            FileState::Modified(_) => {
                // Lógica para determinar se é uma ameaça
                if registry.is_system_hash(&file.current_hash) {
                    registry.update_file_state(file_id, FileState::Clean)?;
                } else {
                    registry.update_file_state(file_id, FileState::Infected(SystemTime::now()))?;
                }
            },
            FileState::Infected(_) => {
                // Automaticamente coloca em quarentena
                registry.update_file_state(file_id, FileState::Quarantined(SystemTime::now()))?;
            },
            _ => {}
        }
        
        Ok(())
    }
}

/// API pública do núcleo do sistema
pub struct SecurityCore;

impl SecurityCore {
    pub fn initialize() {
        // Inicializa os componentes estáticos
        lazy_static::initialize(&FILE_REGISTRY);
        lazy_static::initialize(&ID_GENERATOR);
    }
    
    pub fn monitor_file(path: &str) -> Result<FileID, String> {
        let path = Path::new(path);
        let is_system_file = Self::is_windows_system_file(path);
        
        let mut registry = FILE_REGISTRY.lock().unwrap();
        registry.add_file(path, is_system_file)
    }
    
    pub fn scan_file(file_id: &FileID) -> Result<bool, String> {
        let mut registry = FILE_REGISTRY.lock().unwrap();
        let has_changes = registry.check_for_changes(file_id)?;
        
        if has_changes {
            StateTransitionSystem::handle_file_change(&mut registry, file_id)?;
        }
        
        Ok(has_changes)
    }
    
    pub fn get_file_state(file_id: &FileID) -> Result<FileState, String> {
        let registry = FILE_REGISTRY.lock().unwrap();
        registry.files.get(file_id)
            .map(|f| f.state.clone())
            .ok_or("ID de arquivo não encontrado".to_string())
    }
    
    fn is_windows_system_file(path: &Path) -> bool {
        // Verifica se o arquivo está em um diretório do sistema Windows
        if let Some(path_str) = path.to_str() {
            path_str.to_lowercase().contains("windows\\system32") ||
            path_str.to_lowercase().contains("windows\\syswow64") ||
            path_str.to_lowercase().contains("program files")
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_id_generation() {
        let mut gen = UniqueIDGenerator::new();
        let id1 = gen.generate();
        let id2 = gen.generate();
        
        assert_eq!(id1, "e-b001.000.000.000");
        assert_eq!(id2, "e-b001.000.000.001");
    }
    
    #[test]
    fn test_file_monitoring() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "Conteúdo inicial").unwrap();
        
        SecurityCore::initialize();
        let file_id = SecurityCore::monitor_file(file_path.to_str().unwrap()).unwrap();
        
        // Primeira verificação - deve estar limpo
        assert!(SecurityCore::scan_file(&file_id).unwrap());
        assert_eq!(SecurityCore::get_file_state(&file_id).unwrap(), FileState::Clean);
        
        // Modifica o arquivo
        writeln!(file, "Modificação maliciosa").unwrap();
        file.sync_all().unwrap();
        
        // Segunda verificação - deve detectar modificação
        assert!(SecurityCore::scan_file(&file_id).unwrap());
        let state = SecurityCore::get_file_state(&file_id).unwrap();
        
        match state {
            FileState::Modified(_) => assert!(true),
            _ => assert!(false, "Estado deveria ser Modified"),
        }
    }
}