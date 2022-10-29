use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, SwigResponse};

use crate::state::{config, State, config_read, Imbiber, imbiber, imbiber_read};
use cosmwasm_std::{
    entry_point, to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdResult,
    SubMsgResult, WasmQuery, WasmMsg, Coin, Addr, QueryRequest, Uint128, CosmosMsg, SubMsg,
};

use area52_portal_bm::msg::QueryMsg as PortalQueryMsg;
use area52_portal_bm::msg::ExecuteMsg as PortalExecuteMsg;

use area52_portal_bm::species::{Traveler, Species, sapience_value, SapienceResponse,};
use sha3::{Digest, Keccak256};

static DEFAULT_NUMBER_OF_SWIGS: u8 = 3;
static SECTION31_CONTRACT_ADDR: &str = "wasm_secret_address_do_not_reveal_to_anyone";

#[entry_point]
pub fn reply(_deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.result {
        SubMsgResult::Ok(_) => Ok(Response::default()),
        SubMsgResult::Err(_) => Err(ContractError::NothingToSeeHere {}),
    }
}

/////////////////////////////////////////////////////////////////////////////////////////////////////
#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<cosmwasm_std::Binary> {
    match msg {
        QueryMsg::NumberOfSwigs {} => to_binary(&number_of_swigs(deps)?),
    }
}

pub fn number_of_swigs(deps: Deps) -> StdResult<SwigResponse> {
    let state = config_read(deps.storage).load()?;
    let swigs = state.swigs;
    Ok(SwigResponse { swigs: swigs })
}

/////////////////////////////////////////////////////////////////////////////////////////////////////
#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::ImbibePotion { name, species } => imbibe_potion(name, species, deps, info),
        ExecuteMsg::StepThroughJumpRing {
            portal,
            destination,
            traveler,
        } => step_through_jumpring(portal, destination, traveler, deps, info),
    }
}


pub fn check_sent_required_payment(
    sent: &[Coin],
    required: Option<Coin>,
) -> Result<(), ContractError> {
    if let Some(required_coin) = required {
        let required_amount = required_coin.amount.u128();
        if required_amount > 0 {
            let sent_sufficient_funds = sent.iter().any(|coin| {
                // check if a given sent coin matches denom and has sufficient amount
                // .any is a function which when passed an iterator, will return true if any element satisfies the predicate.
                coin.denom == required_coin.denom && coin.amount.u128() >= required_amount
            });

            if sent_sufficient_funds {
                return Ok(());
            } else {
                return Err(ContractError::NotEnoughFunds {});
            }
        }
    }
    Ok(())
}

pub fn check_sapience_level(
    portal: &Addr,
    deps: &DepsMut,
    info: &MessageInfo,
) -> Result<Response, ContractError> {
    let query = WasmQuery::Smart {
        contract_addr: portal.to_string(),
        msg: to_binary(&PortalQueryMsg::MinimumSapience {})?,
    };
    let res: SapienceResponse = deps.querier.query(&QueryRequest::Wasm(query))?;

    let key = info.sender.as_bytes();
    let imbiber = imbiber_read(deps.storage).load(key).unwrap();
    let species_sapience = imbiber.species.sapience_level;

    if sapience_value(&res.level) < sapience_value(&species_sapience) {
        return Err(ContractError::NotSapientEnough {});
    };
    Ok(Response::default())
}

pub fn step_through_jumpring(
    portal: Addr,
    destination: Addr,
    traveler: Traveler,
    deps: DepsMut,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    check_sapience_level(&portal, &deps, &info)?;

    if traveler.cyberdized != true {
        return Err(ContractError::NotACyborg {});
    }

    let required_payment = Coin {
        denom: "PORT".to_string(),
        amount: Uint128::from(1u128),
    };
    check_sent_required_payment(&info.funds, Some(required_payment))?;

    let msg = WasmMsg::Execute {
        contract_addr: portal.to_string(),
        msg: to_binary(&PortalExecuteMsg::JumpRingTravel { to: destination }).unwrap(),
        funds: vec![],
    };

    Ok(Response::new().add_message(msg))
}



pub fn cyborg_dna_generator(value: &String, dna_length: usize, dna_modulus: u8) -> Vec<u8> {
    let mut hasher = Keccak256::new();
    hasher.update(value);

    let result = hasher.finalize();
    let slice = &result[0..dna_length];
    let mut truncated = Vec::with_capacity(dna_length);

    for item in slice {
        truncated.push(item % &dna_modulus);
    }

    return truncated;
}



pub fn imbibe_potion(
    name: String,
    species: Species,
    deps: DepsMut,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let mut state = config(deps.storage).load()?;
    let swigs = state.swigs;
    if swigs == 0 {
        return Err(ContractError::OutOfSwigs {});
    }

    state.swigs = swigs - 1;
    config(deps.storage).save(&state)?;

    let cyborg_dna = cyborg_dna_generator(
        &info.sender.to_string(),
        state.dna_length,
        state.dna_modulus,
    );

    let cyborg = Imbiber {
        address: info.sender.clone(),
        species: species.clone(),
        name: name.clone(),
        cyborg_dna: cyborg_dna,
    };

    let key = info.sender.as_bytes();
    imbiber(deps.storage).save(key, &cyborg)?;

    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: SECTION31_CONTRACT_ADDR.to_string(),
        msg: to_binary(&PortalExecuteMsg::Snitch {
            address: info.sender,
            name: name,
            species: species,
        })?,
        funds: vec![],
    });

    let submsg = SubMsg::reply_on_error(msg, 1);

    Ok(Response::new().add_submessage(submsg))
}

/////////////////////////////////////////////////////////////////////////////////////////////////////
#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let default_number_swigs = DEFAULT_NUMBER_OF_SWIGS;
    let state = State {
        owner: info.sender,
        dna_length: msg.dna_length,
        dna_modulus: msg.dna_modulus,
        swigs: default_number_swigs,
    };
    config(deps.storage).save(&state)?;
    Ok(Response::default())
}