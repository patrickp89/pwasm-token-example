#![cfg_attr(not(feature="std"), no_main)]
#![cfg_attr(not(feature="std"), no_std)]

#![feature(proc_macro)]
#![feature(alloc)]
#![allow(non_snake_case)]

extern crate tiny_keccak;
extern crate alloc;
extern crate bigint;
extern crate parity_hash;
extern crate pwasm_std;
extern crate pwasm_ethereum;
extern crate pwasm_abi;
extern crate pwasm_abi_derive;

use alloc::vec::Vec;

use tiny_keccak::Keccak;
use pwasm_ethereum::{storage, ext};
use pwasm_std::hash::{Address, H256};
use bigint::U256;
use pwasm_abi_derive::eth_abi;

// TokenContract is an interface definition of a contract.
// The current example covers the minimal subset of ERC20 token standard.
// eth_abi macro parses an interface (trait) definition of a contact and generates
// two structs: Endpoint and Client.
//
// Endpoint is an entry point for contract calls.
// eth_abi macro generates a table of Method IDs corresponding with every method signature defined in the trait
// and defines it statically in the generated code.
// Scroll down at "pub fn call(desc: *mut u8)" to see how
// Endpoint instantiates with a struct TokenContractInstance which implements the trait definition.
//
// Client is a struct which is useful for call generation to a deployed contract. For example:
// ```
//     let mut client = Client::new(contactAddress);
//     let balance = client
//        .value(someValue) // you can attach some value for a call optionally
//        .balanceOf(someAddress);
// ```
// Will generate a Solidity-compatible call for the contract, deployed on `contactAddress`.
// Then it invokes pwasm_std::ext::call on `contactAddress` and returns the result.
#[eth_abi(Endpoint, Client)]
pub trait TokenContract {
	fn constructor(&mut self, _total_supply: U256);

	/// What is the balance of a particular account?
	#[constant]
	fn balanceOf(&mut self, _owner: Address) -> U256;

	/// Total amount of tokens
	#[constant]
	fn totalSupply(&mut self) -> U256;

	/// Transfer the balance from owner's account to another account
	fn transfer(&mut self, _to: Address, _amount: U256) -> bool;

	/// Send _value amount of tokens from address _from to address _to
	/// The transferFrom method is used for a withdraw workflow, allowing contracts to send
	/// tokens on your behalf, for example to "deposit" to a contract address and/or to charge
	/// fees in sub-currencies; the command should fail unless the _from account has
	/// deliberately authorized the sender of the message via some mechanism; we propose
	/// these standardized APIs for approval:
	fn transferFrom(&mut self, _from: Address, _to: Address, _amount: U256) -> bool;

	/// Allow _spender to withdraw from your account, multiple times, up to the _value amount.
	/// If this function is called again it overwrites the current allowance with _value.
	fn approve(&mut self, _spender: Address, _value: U256) -> bool;

	/// Check the amount of tokens spender have right to spend on behalf of owner
	fn allowance(&mut self, _owner: Address, _spender: Address) -> U256;

	#[event]
	fn Transfer(&mut self, indexed_from: Address, indexed_to: Address, _value: U256);
	#[event]
	fn Approval(&mut self, indexed_owner: Address, indexed_spender: Address, _value: U256);
}

static TOTAL_SUPPLY_KEY: H256 = H256([2,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]);
static OWNER_KEY: H256 = H256([3,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]);

// Reads balance by address
fn read_balance_of(owner: &Address) -> U256 {
	storage::read(&balance_key(owner)).into()
}

// Reads allowance value using key
// Key generated by allowance_key function
fn read_allowance(key: &H256) -> U256 {
	storage::read(key).into()
}

// Writes allowance value
// Key generated by allowance_key function
fn write_allowance(key: &H256, value: U256) {
	storage::write(key, &value.into())
}

// Generates the "allowance" storage key to map owner and spender
fn allowance_key(owner: &Address, spender: &Address) -> H256 {
	let mut keccak = Keccak::new_keccak256();
	let mut res = H256::new();
	keccak.update("allowance_key".as_ref());
	keccak.update(owner.as_ref());
	keccak.update(spender.as_ref());
	keccak.finalize(&mut res);
	res
}

// Generates a balance key for some address.
// Used to map balances with their owners.
fn balance_key(address: &Address) -> H256 {
	let mut key = H256::from(address);
	key[0] = 1; // just a naiive "namespace";
	key
}

pub struct TokenContractInstance;

impl TokenContract for TokenContractInstance {
	fn constructor(&mut self, total_supply: U256) {
		let sender = ext::sender();
		// Set up the total supply for the token
		storage::write(&TOTAL_SUPPLY_KEY, &total_supply.into());
		// Give all tokens to the contract owner
		storage::write(&balance_key(&sender), &total_supply.into());
		// Set the contract owner
		storage::write(&OWNER_KEY, &H256::from(sender).into());
	}

	fn balanceOf(&mut self, owner: Address) -> U256 {
		read_balance_of(&owner)
	}

	fn totalSupply(&mut self) -> U256 {
		storage::read(&TOTAL_SUPPLY_KEY).into()
	}

	fn transfer(&mut self, to: Address, amount: U256) -> bool {
		let sender = ext::sender();
		let senderBalance = read_balance_of(&sender);
		let recipientBalance = read_balance_of(&to);
		if amount == 0.into() || senderBalance < amount {
			false
		} else {
			let new_sender_balance = senderBalance - amount;
			let new_recipient_balance = recipientBalance + amount;
			// TODO: impl From<U256> for H256 makes convertion to big endian. Could be optimized
			storage::write(&balance_key(&sender), &new_sender_balance.into());
			storage::write(&balance_key(&to), &new_recipient_balance.into());
			self.Transfer(sender, to, amount);
			true
		}
	}

	fn approve(&mut self, spender: Address, value: U256) -> bool {
		write_allowance(&allowance_key(&ext::sender(), &spender), value);
		self.Approval(ext::sender(), spender, value);
		true
	}

	fn allowance(&mut self, owner: Address, spender: Address) -> U256 {
		read_allowance(&allowance_key(&owner, &spender))
	}

	fn transferFrom(&mut self, from: Address, to: Address, amount: U256) -> bool {
		let fromBalance = read_balance_of(&from);
		let recipientBalance = read_balance_of(&to);
		let a_key = allowance_key(&from, &ext::sender());
		let allowed = read_allowance(&a_key);
		if  allowed < amount || amount == 0.into() || fromBalance < amount {
			false
		} else {
			let new_allowed = allowed - amount;
			let new_from_balance = fromBalance - amount;
			let new_recipient_balance = recipientBalance + amount;
			storage::write(&a_key, &new_allowed.into());
			storage::write(&balance_key(&from), &new_from_balance.into());
			storage::write(&balance_key(&to), &new_recipient_balance.into());
			self.Transfer(from, to, amount);
			true
		}
	}
}

#[cfg(test)]
#[macro_use]
extern crate pwasm_test;

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    extern crate std;
    use super::*;
    use pwasm_test::{External, ExternalBuilder, ExternalInstance, get_external, set_external};
    use bigint::U256;
    use pwasm_std::hash::{Address};

    test_with_external!(
        ExternalBuilder::new()
            .storage([1,0,0,0,0,0,0,0,0,0,0,0,
                            31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31].into(), U256::from(100000).into())
            .build(),
        balanceOf_should_return_balance {
            let address = Address::from([31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31]);
            let mut contract = TokenContractInstance{};
            assert_eq!(contract.balanceOf(address), 100000.into())
        }
    );

    test_with_external!(
        ExternalBuilder::new().build(),
        totalSupply_should_return_total_supply_contract_was_initialized_with {
            let mut contract = TokenContractInstance{};
            let total_supply = 42.into();
            contract.constructor(total_supply);
            assert_eq!(contract.totalSupply(), total_supply);
        }
    );

    test_with_external!(
        ExternalBuilder::new().build(),
        should_succeed_in_creating_max_possible_amount_of_tokens {
            let mut contract = TokenContractInstance{};
            // set total supply to maximum value of an unsigned 256 bit integer
            let total_supply = U256::from_dec_str("115792089237316195423570985008687907853269984665640564039457584007913129639935").unwrap();
            assert_eq!(total_supply, U256::max_value());
            contract.constructor(total_supply);
            assert_eq!(contract.totalSupply(), total_supply);
        }
    );

    test_with_external!(
        ExternalBuilder::new().build(),
        should_initially_give_the_total_supply_to_the_creator {
            let mut contract = TokenContractInstance{};
            let total_supply = 10000.into();
            contract.constructor(total_supply);
            assert_eq!(
                contract.balanceOf(get_external::<ExternalInstance>().sender()),
                total_supply);
        }
    );

    #[test]
    fn should_succeed_transfering_1000_from_owner_to_another_address() {
        let mut contract = TokenContractInstance{};

        let owner_address = Address::from("0xea674fdde714fd979de3edf0f56aa9716b898ec8");
        let sam_address = Address::from("0xdb6fd484cfa46eeeb73c71edee823e4812f9e2e1");

        set_external(Box::new(ExternalBuilder::new()
            .sender(owner_address.clone())
            .build()));

        let total_supply = 10000.into();
        contract.constructor(total_supply);

        assert_eq!(contract.balanceOf(owner_address), total_supply);

        assert_eq!(contract.transfer(sam_address, 1000.into()), true);
        assert_eq!(get_external::<ExternalInstance>().logs().len(), 1);
        assert_eq!(get_external::<ExternalInstance>().logs()[0].topics.as_ref(), &[
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(), // hash of the event name
            "0x000000000000000000000000ea674fdde714fd979de3edf0f56aa9716b898ec8".into(), // sender address
            "0x000000000000000000000000db6fd484cfa46eeeb73c71edee823e4812f9e2e1".into()]); // recipient address
        assert_eq!(get_external::<ExternalInstance>().logs()[0].data.as_ref(), &[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 232]);
        assert_eq!(contract.balanceOf(owner_address), 9000.into());
        assert_eq!(contract.balanceOf(sam_address), 1000.into());
    }

    #[test]
    fn should_return_false_transfer_not_sufficient_funds() {
        set_external(Box::new(ExternalBuilder::new().build()));
        let mut contract = TokenContractInstance{};
        contract.constructor(10000.into());
        assert_eq!(contract.transfer("0xdb6fd484cfa46eeeb73c71edee823e4812f9e2e1".into(), 50000.into()), false);
        assert_eq!(contract.balanceOf(::pwasm_ethereum::ext::sender()), 10000.into());
        assert_eq!(contract.balanceOf("0xdb6fd484cfa46eeeb73c71edee823e4812f9e2e1".into()), 0.into());
        assert_eq!(get_external::<ExternalInstance>().logs().len(), 0, "Should be no events created");
    }

    test_with_external!(
        ExternalBuilder::new().build(),
        approve_should_approve {
            let mut contract = TokenContractInstance{};
            let spender: Address = "0xdb6fd484cfa46eeeb73c71edee823e4812f9e2e1".into();
            contract.constructor(40000.into());
            contract.approve(spender, 40000.into());
            assert_eq!(get_external::<ExternalInstance>().logs().len(), 1, "Should be 1 event logged");
            assert_eq!(get_external::<ExternalInstance>().logs()[0].topics.as_ref(), &[
                "0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925".into(), // hash of the event name
                "0x0000000000000000000000000000000000000000000000000000000000000000".into(), // sender (owner) address
                "0x000000000000000000000000db6fd484cfa46eeeb73c71edee823e4812f9e2e1".into()]); // spender address
            assert_eq!(contract.allowance(::pwasm_ethereum::ext::sender(), spender.clone()), 40000.into());
        }
    );

    test_with_external!(
        ExternalBuilder::new().build(),
        spender_should_be_able_to_spend_if_allowed {
            let mut contract = TokenContractInstance{};
            let owner: Address = Address::new();
            let spender: Address = "0xdb6fd484cfa46eeeb73c71edee823e4812f9e2e1".into();
            let samAddress: Address = "0xea674fdde714fd979de3edf0f56aa9716b898ec8".into();
            contract.constructor(40000.into());
            contract.approve(spender, 10000.into());

            // Build different external with sender = spender
            let spenderExternal = ExternalBuilder::from(get_external::<ExternalInstance>())
                .sender(spender)
                .build();
            set_external(Box::new(spenderExternal));

            assert_eq!(contract.transferFrom(owner.clone(), samAddress.clone(), 5000.into()), true);
            assert_eq!(contract.balanceOf(samAddress.clone()), 5000.into());
            assert_eq!(contract.balanceOf(owner.clone()), 35000.into());

            assert_eq!(contract.transferFrom(owner.clone(), samAddress.clone(), 5000.into()), true);
            assert_eq!(contract.balanceOf(samAddress.clone()), 10000.into());
            assert_eq!(contract.balanceOf(owner.clone()), 30000.into());

            // The limit has reached. No more coins should be available to spend for the spender
            assert_eq!(contract.transferFrom(owner.clone(), samAddress.clone(), 1.into()), false);
            assert_eq!(contract.balanceOf(samAddress.clone()), 10000.into());
            assert_eq!(contract.balanceOf(owner.clone()), 30000.into());
            assert_eq!(get_external::<ExternalInstance>().logs().len(), 2, "Two events should be created");
        }
    );

    test_with_external!(
        ExternalBuilder::new().build(),
        spender_should_not_be_able_to_spend_if_owner_has_no_coins {
            let mut contract = TokenContractInstance{};
            let owner: Address = Address::new();
            let spender: Address = "0xdb6fd484cfa46eeeb73c71edee823e4812f9e2e1".into();
            let samAddress: Address = "0xea674fdde714fd979de3edf0f56aa9716b898ec8".into();
            contract.constructor(70000.into());
            contract.transfer(samAddress, 30000.into());
            contract.approve(spender, 40000.into());

            // Build different external with sender = spender
            let spenderExternal = ExternalBuilder::from(get_external::<ExternalInstance>())
                .sender(spender)
                .build();
            set_external(Box::new(spenderExternal));

            // Despite of the allowance, can't transfer because the owner is out of tokens
            assert_eq!(contract.transferFrom(owner.clone(), samAddress.clone(), 40001.into()), false);
            assert_eq!(contract.balanceOf(samAddress.clone()), 30000.into());
            assert_eq!(contract.balanceOf(owner.clone()), 40000.into());
            assert_eq!(get_external::<ExternalInstance>().logs().len(), 0, "Should be no events created");
        }
    );
}
