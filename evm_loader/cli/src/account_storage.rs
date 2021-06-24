use evm::backend::Apply;
use evm::{H160, U256};
#[allow(unused)]
use solana_sdk::{
    pubkey::Pubkey,
    account::Account,
    commitment_config::CommitmentConfig,
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    program::invoke_signed,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap};
use evm_loader::{
    account_data::AccountData,
    solana_backend::AccountStorage,
    solidity_account::SolidityAccount,
};
use std::borrow::BorrowMut;
use std::cell::RefCell; 
use std::rc::Rc;
use crate::Config;
#[allow(unused)]
use solana_program::instruction::Instruction;
use evm_loader::solana_backend::SolanaBackend;

#[derive(Serialize, Deserialize, Debug)]
pub struct AccountJSON {
    address: String,
    account: String,
    contract: Option<String>,
    writable: bool,
    new: bool,
    code_size: Option<usize>,
}

struct SolanaAccount {
    account: Account,
    code_account: Option<Account>,
    key: Pubkey,
    writable: bool,
    code_size: Option<usize>,
}

struct SolanaNewAccount {
    key: Pubkey,
    writable: bool,
    code_size: Option<usize>
}

impl SolanaAccount {
    pub fn new(account: Account, key: Pubkey, code_account: Option<Account>) -> Self {
        eprintln!("SolanaAccount::new");
        Self{account, key, writable: false, code_account, code_size: None}
    }
}

impl SolanaNewAccount {
    pub const fn new(key: Pubkey) -> Self {
        Self{key, writable: false, code_size: None}
    }
}

#[allow(clippy::module_name_repetitions)]
pub struct EmulatorAccountStorage<'a> {
    accounts: RefCell<HashMap<H160, SolanaAccount>>,
    new_accounts: RefCell<HashMap<H160, SolanaNewAccount>>,
    config: &'a Config,
    contract_id: H160,
    caller_id: H160,
    block_number: u64,
    block_timestamp: i64,
}

impl<'a> EmulatorAccountStorage<'a> {
    pub fn new(config: &'a Config, contract_id: H160, caller_id: H160) -> EmulatorAccountStorage {
        eprintln!("backend::new");

        let slot = if let Ok(slot) = config.rpc_client.get_slot() {
            eprintln!("Got slot");
            eprintln!("Slot {}", slot);
            slot
        }
        else {
            eprintln!("Get slot error");
            0
        };

        let timestamp = if let Ok(timestamp) = config.rpc_client.get_block_time(slot) {
            eprintln!("Got timestamp");
            eprintln!("timestamp {}", timestamp);
            timestamp
        } else {
            eprintln!("Get timestamp error");
            0
        };

        Self {
            accounts: RefCell::new(HashMap::new()),
            new_accounts: RefCell::new(HashMap::new()),
            config,
            contract_id,
            caller_id,
            block_number: slot,
            block_timestamp: timestamp,
        }
    }

    pub fn get_account_from_solana(config: &'a Config, address: &H160) -> Option<(Account, Option<Account>)> {
        let solana_address =  Pubkey::find_program_address(&[&address.to_fixed_bytes()], &config.evm_loader).0;
        eprintln!("Not found account for 0x{} => {}", &hex::encode(&address.as_fixed_bytes()), &solana_address.to_string());

        if let Some(acc) = config.rpc_client.get_account_with_commitment(&solana_address, CommitmentConfig::processed()).unwrap().value {
            eprintln!("Account found");
            eprintln!("Account data len {}", acc.data.len());
            eprintln!("Account owner {}", acc.owner.to_string());

            let account_data = match AccountData::unpack(&acc.data) {
                Ok(acc_data) => match acc_data {
                    AccountData::Account(acc) => acc,
                    _ => return None,
                },
                Err(_) => return None,
            };

            let code_account = if account_data.code_account == Pubkey::new_from_array([0_u8; 32]) {
                eprintln!("code_account == Pubkey::new_from_array([0u8; 32])");
                None
            } else {
                eprintln!("code_account != Pubkey::new_from_array([0u8; 32])");
                eprintln!("account key:  {}", &solana_address.to_string());
                eprintln!("code account: {}", &account_data.code_account.to_string());

                if let Some(acc) = config.rpc_client.get_account_with_commitment(&account_data.code_account, CommitmentConfig::processed()).unwrap().value {
                    eprintln!("Account found");
                    Some(acc)
                }
                else {
                    eprintln!("Account not found");
                    None
                }
            };

            Some((acc, code_account))
        }
        else {
            eprintln!("Account not found {}", &address.to_string());

            None
        }
    }

    fn create_acc_if_not_exists(&self, address: &H160) -> bool {
        let mut accounts = self.accounts.borrow_mut(); 
        let mut new_accounts = self.new_accounts.borrow_mut(); 
        if accounts.get(address).is_none() {
            let solana_address =  Pubkey::find_program_address(&[&address.to_fixed_bytes()], &self.config.evm_loader).0;
            if let Some((acc, code_account)) = Self::get_account_from_solana(self.config, address) {
                accounts.insert(*address, SolanaAccount::new(acc, solana_address, code_account));
                true
            }
            else {
                eprintln!("Account not found {}", &address.to_string());
                new_accounts.insert(*address, SolanaNewAccount::new(solana_address));
                false
            }
        } else {
            true
        }
    }

    // pub fn make_solidity_account<'a>(self, account:&'a SolanaAccount) -> SolidityAccount<'a> {
    //     let mut data = account.account.data.clone();
    //     let data_rc: std::rc::Rc<std::cell::RefCell<&mut [u8]>> = Rc::new(RefCell::new(&mut data));
    //     SolidityAccount::new(&account.key, data_rc, account.account.lamports).unwrap()
    // }

    pub fn apply<A, I>(&self, values: A)
            where
                A: IntoIterator<Item=Apply<I>>,
                I: IntoIterator<Item=(U256, U256)>,
    {             
        let mut accounts = self.accounts.borrow_mut(); 
        let mut new_accounts = self.new_accounts.borrow_mut();

        for apply in values {
            match apply {
                Apply::Modify {address, basic, code, storage: _, reset_storage} => {
                    if let Some(acc) = accounts.get_mut(&address) {
                        *acc.writable.borrow_mut() = true;
                        *acc.code_size.borrow_mut() = code.map(|v| v.len());
                    } else if let Some(acc) = new_accounts.get_mut(&address) {
                        *acc.code_size.borrow_mut() = code.map(|v| v.len());
                        *acc.writable.borrow_mut() = true;
                    } else {
                        eprintln!("Account not found {}", &address.to_string());
                    }
                    eprintln!("Modify: {} {} {} {}", &address.to_string(), &basic.nonce.as_u64(), &basic.balance.as_u64(), &reset_storage.to_string());
                },
                Apply::Delete {address: addr} => {
                    eprintln!("Delete: {}", addr.to_string());
                },
            }
        };
    }

    pub fn get_used_accounts(&self) -> Vec<AccountJSON>
    {
        let mut arr = Vec::new();

        let accounts = self.accounts.borrow();
        for (address, acc) in accounts.iter() {
            let solana_address = Pubkey::find_program_address(&[&address.to_fixed_bytes()], &self.config.evm_loader).0;

            let contract_address = {
                let addr = AccountData::unpack(&acc.account.data).unwrap().get_account().unwrap().code_account;
                if addr == Pubkey::new_from_array([0_u8; 32]) {
                    None
                } else {
                    Some(addr)
                }
            };

            if !SolanaBackend::<EmulatorAccountStorage>::is_system_address(&address) {
                arr.push(AccountJSON{
                        address: "0x".to_string() + &hex::encode(&address.to_fixed_bytes()),
                        writable: acc.writable,
                        new: false,
                        account: solana_address.to_string(),
                        contract: contract_address.map(|v| v.to_string()),
                        code_size: acc.code_size,
                });
            }
        }

        let new_accounts = self.new_accounts.borrow();
        for (address, acc) in new_accounts.iter() {
            if !SolanaBackend::<EmulatorAccountStorage>::is_system_address(&address) {
                arr.push(AccountJSON{
                        address: "0x".to_string() + &hex::encode(&address.to_fixed_bytes()),
                        writable: acc.writable,
                        new: true,
                        account: acc.key.to_string(),
                        contract: None,
                        code_size: acc.code_size,
                });
            }
        }

        arr
    }
}

impl<'a> AccountStorage for EmulatorAccountStorage<'a> {
    fn apply_to_account<U, D, F>(&self, address: &H160, d: D, f: F) -> U
    where F: FnOnce(&SolidityAccount) -> U,
          D: FnOnce() -> U
    {
        self.create_acc_if_not_exists(address);
        let accounts = self.accounts.borrow();
        match accounts.get(address) {
            None => d(),
            Some(acc) => {
                let account_data = match AccountData::unpack(&acc.account.data) {
                    Ok(acc_data) => match acc_data {
                        AccountData::Account(_) => acc_data,
                        _ => return d(),
                    },
                    Err(_) => return d(),
                };
                if acc.code_account.is_some() {
                    let mut code_data = acc.code_account.as_ref().unwrap().data.clone();
                    let contract_data = match AccountData::unpack(&code_data) {
                        Ok(acc_data) => match acc_data {
                            AccountData::Contract(_) => acc_data,
                            _ => return d(),
                        },
                        Err(_) => return d(),
                    };
                    let code_data: std::rc::Rc<std::cell::RefCell<&mut [u8]>> = Rc::new(RefCell::new(&mut code_data));
                    let account = SolidityAccount::new(&acc.key, acc.account.lamports, account_data, Some((contract_data, code_data)));
                    f(&account)
                } else {
                    let account = SolidityAccount::new(&acc.key, acc.account.lamports, account_data, None);
                    f(&account)
                }
            },
        }
    }

    fn contract(&self) -> H160 { self.contract_id }

    fn origin(&self) -> H160 { self.caller_id }

    fn block_number(&self) -> U256 { self.block_number.into() }

    fn block_timestamp(&self) -> U256 { self.block_timestamp.into() }

    fn external_call(
        &self,
        instruction: &Instruction,
        account_infos: &[AccountInfo]
    ) -> ProgramResult {
        eprintln!("emulate external_call");
        // Ok(())
        let (contract_eth, contract_nonce) = self.seeds(&self.contract()).unwrap();   // do_call already check existence of Ethereum account with such index
        let contract_seeds = [contract_eth.as_bytes(), &[contract_nonce]];

        match self.seeds(&self.origin()) {
            Some((sender_eth, sender_nonce)) => {
                let sender_seeds = [sender_eth.as_bytes(), &[sender_nonce]];
                invoke_signed(
                    instruction,
                    account_infos,
                    &[&sender_seeds[..], &contract_seeds[..]]
                )
            }
            None => {
                invoke_signed(
                    instruction,
                    account_infos,
                    &[&contract_seeds[..]]
                )
            }
        }
    }
}
