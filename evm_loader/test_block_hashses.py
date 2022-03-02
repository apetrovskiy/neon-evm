from solana.publickey import PublicKey
from solana.transaction import AccountMeta, TransactionInstruction, Transaction
from spl.token.instructions import get_associated_token_address
from spl.token.constants import TOKEN_PROGRAM_ID, ACCOUNT_LEN
import unittest
from eth_utils import abi
from base58 import b58decode
import random

from eth_tx_utils import make_keccak_instruction_data, make_instruction_data_from_tx, JsonEncoder
from solana_utils import *

CONTRACTS_DIR = os.environ.get("CONTRACTS_DIR", "evm_loader/")
evm_loader_id = os.environ.get("EVM_LOADER")
ETH_TOKEN_MINT_ID: PublicKey = PublicKey(os.environ.get("ETH_TOKEN_MINT"))
holder_id = 0

class PrecompilesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        print("\ntest_block_hashses.py setUpClass")

        cls.token = SplToken(solana_url)
        wallet = OperatorAccount(operator1_keypair_path())
        cls.loader = EvmLoader(wallet, evm_loader_id)
        cls.acc = wallet.get_acc()

        # Create ethereum account for user account
        cls.caller_ether = eth_keys.PrivateKey(cls.acc.secret_key()).public_key.to_canonical_address()
        (cls.caller, cls.caller_nonce) = cls.loader.ether2program(cls.caller_ether)
        cls.caller_token = get_associated_token_address(PublicKey(cls.caller), ETH_TOKEN_MINT_ID)

        if getBalance(cls.caller) == 0:
            print("Create caller account...")
            _ = cls.loader.createEtherAccount(cls.caller_ether)
            cls.token.transfer(ETH_TOKEN_MINT_ID, 201, get_associated_token_address(PublicKey(cls.caller), ETH_TOKEN_MINT_ID))
            print("Done\n")

        print('Account:', cls.acc.public_key(), bytes(cls.acc.public_key()).hex())
        print("Caller:", cls.caller_ether.hex(), cls.caller_nonce, "->", cls.caller,
              "({})".format(bytes(PublicKey(cls.caller)).hex()))

        print("deploy contract: ")
        (cls.owner_contract, cls.eth_contract, cls.contract_code) = cls.loader.deployChecked(
                CONTRACTS_DIR+'BlockHashTest.binary',
                cls.caller,
                cls.caller_ether
            )
        print("contract id: ", cls.owner_contract, cls.eth_contract)
        print("code id: ", cls.contract_code)

        collateral_pool_index = 2
        cls.collateral_pool_address = create_collateral_pool_address(collateral_pool_index)
        cls.collateral_pool_index_buf = collateral_pool_index.to_bytes(4, 'little')

    def send_transaction(self, data):
        trx = self.make_transactions(data)
        result = send_transaction(client, trx, self.acc)
        result = result["result"]
        return b58decode(result['meta']['innerInstructions'][0]['instructions'][-1]['data'])[8+2:].hex()

    def make_transactions(self, call_data):
        eth_tx = {
            'to': self.eth_contract,
            'value': 0,
            'gas': 9999999,
            'gasPrice': 1_000_000_000,
            'nonce': getTransactionCount(client, self.caller),
            'data': call_data,
            'chainId': 111
        }

        (_from_addr, sign, msg) = make_instruction_data_from_tx(eth_tx, self.acc.secret_key())
        trx_data = self.caller_ether + sign + msg
        keccak_instruction = make_keccak_instruction_data(1, len(msg), 5)
        
        solana_trx = Transaction().add(
                self.sol_instr_keccak(keccak_instruction) 
            ).add( 
                self.sol_instr_call(trx_data) 
            )

        return solana_trx

    def sol_instr_keccak(self, keccak_instruction):
        return  TransactionInstruction(program_id="KeccakSecp256k11111111111111111111111111111", data=keccak_instruction, keys=[
                    AccountMeta(pubkey=self.caller, is_signer=False, is_writable=False),
                ])

    def sol_instr_call(self, trx_data):
        neon_evm_instr_05_single = create_neon_evm_instr_05_single(
            self.loader.loader_id,
            self.caller,
            self.acc.public_key(),
            self.owner_contract,
            self.contract_code,
            self.collateral_pool_index_buf,
            self.collateral_pool_address,
            trx_data,
            add_meta=[AccountMeta(pubkey=self.block_hash_source, is_signer=False, is_writable=False),]
        )
        print('neon_evm_instr_05_single:', neon_evm_instr_05_single)
        return neon_evm_instr_05_single

    def make_getCurrentValues(self):
        return abi.function_signature_to_4byte_selector('getCurrentValues()')

    def make_getValues(self, number: int):
        return abi.function_signature_to_4byte_selector('getValues(uint number)')\
                + bytes.fromhex("%064x" % number)

    def get_blocks_from_solana(self):
        slot_hash = {}
        current_slot = client.get_slot()["result"]
        for slot in range(current_slot):
            hash_val = base58.b58decode(client.get_confirmed_block(slot)['result']['blockhash']).hex()
            print(f"slot: {slot} hash_val: {hash_val}")
            slot_hash[int(slot)] = hash_val
        return slot_hash

    def test_01_block_hashes(self):
        print("test_01_block_hashes")
        solana_result = self.get_blocks_from_solana()
        for i in range(6):
            if i % 2 == 0:
                self.block_hash_source = "SysvarRecentB1ockHashes11111111111111111111"
            else:
                self.block_hash_source = "SysvarS1otHashes111111111111111111111111111"
            sol_slot, sol_hash = random.choice(list(solana_result.items()))
            print(self.make_getValues(sol_slot).hex())
            result = self.send_transaction(self.make_getValues(sol_slot))
            print(f"{self.block_hash_source} sol_slot: {sol_slot} sol_hash: {sol_hash} result: {result}")

    def test_02_block_hashes(self):
        print("test_02_block_hashes")
        self.block_hash_source = "SysvarRecentB1ockHashes11111111111111111111"
        result = self.send_transaction(self.make_getCurrentValues())
        print(f"{self.block_hash_source} result: {result}")
        self.block_hash_source = "SysvarS1otHashes111111111111111111111111111"
        result = self.send_transaction(self.make_getCurrentValues())
        print(f"{self.block_hash_source} result: {result}")


if __name__ == '__main__':
    unittest.main()
