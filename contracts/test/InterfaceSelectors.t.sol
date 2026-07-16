// contracts/test: lock the Phase 1E function, error, event, enum, struct, and constructor shapes.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {BridgeTimelockController} from "../src/BridgeTimelockController.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {IERC20Metadata} from "@openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol";
import {IERC5267} from "@openzeppelin/contracts/interfaces/IERC5267.sol";

contract BridgeConstructorFixture {
    bytes32 public immutable argumentsHash;

    constructor(
        string memory tokenName,
        string memory tokenSymbol,
        uint8 tokenDecimals,
        address bridgeSigner,
        address runtimeAdministrator,
        address baseAdminTimelock,
        bytes32 approvedTimelockRuntimeCodeHash,
        uint256 perDepositLimit,
        uint256 mintWindowLimit,
        uint64 mintWindowDuration,
        uint256 maxServiceFee,
        uint256 initialServiceFee
    ) {
        argumentsHash = keccak256(
            abi.encode(
                tokenName,
                tokenSymbol,
                tokenDecimals,
                bridgeSigner,
                runtimeAdministrator,
                baseAdminTimelock,
                approvedTimelockRuntimeCodeHash,
                perDepositLimit,
                mintWindowLimit,
                mintWindowDuration,
                maxServiceFee,
                initialServiceFee
            )
        );
    }
}

contract InterfaceTupleFixture {
    function deposit(IBridge.DepositMintRequest calldata) external pure {}

    function withdrawal(IBridge.Withdrawal calldata) external pure {}
}

contract InterfaceSelectorsTest {
    function testBSNSFunctionSelectors() public pure {
        _assertSelector(IBSNS.bridge.selector, "bridge()");
        _assertSelector(IBSNS.bridgeMint.selector, "bridgeMint(address,uint256)");
        _assertSelector(IBSNS.bridgeBurn.selector, "bridgeBurn(uint256)");
        _assertSelector(IBSNS.version.selector, "version()");
        _assertSelector(IERC5267.eip712Domain.selector, "eip712Domain()");
        _assertSelector(IERC20Metadata.name.selector, "name()");
        _assertSelector(IERC20Metadata.symbol.selector, "symbol()");
        _assertSelector(IERC20Metadata.decimals.selector, "decimals()");
        _assertSelector(IERC20.totalSupply.selector, "totalSupply()");
        _assertSelector(IERC20.balanceOf.selector, "balanceOf(address)");
        _assertSelector(IERC20.transfer.selector, "transfer(address,uint256)");
        _assertSelector(IERC20.allowance.selector, "allowance(address,address)");
        _assertSelector(IERC20.approve.selector, "approve(address,uint256)");
        _assertSelector(IERC20.transferFrom.selector, "transferFrom(address,address,uint256)");
    }

    function testBSNSAuthorizationSelectors() public pure {
        _assertSelector(IBSNS.authorizationState.selector, "authorizationState(address,bytes32)");
        _assertSelector(
            IBSNS.transferWithAuthorization.selector,
            "transferWithAuthorization(address,address,uint256,uint256,uint256,bytes32,uint8,bytes32,bytes32)"
        );
        _assertSelector(
            IBSNS.receiveWithAuthorization.selector,
            "receiveWithAuthorization(address,address,uint256,uint256,uint256,bytes32,uint8,bytes32,bytes32)"
        );
        _assertSelector(
            IBSNS.cancelAuthorization.selector, "cancelAuthorization(address,bytes32,uint8,bytes32,bytes32)"
        );
    }

    function testBridgeOperationSelectors() public pure {
        _assertSelector(IBridge.mintDeposit.selector, "mintDeposit((bytes32,address,uint256,uint256,uint256))");
        _assertSelector(IBridge.createWithdrawal.selector, "createWithdrawal(uint256,uint256,bytes,bytes32)");
    }

    function testBridgeViewSelectors() public pure {
        _assertSelector(IBridge.bsns.selector, "bsns()");
        _assertSelector(IBridge.bridgeSigner.selector, "bridgeSigner()");
        _assertSelector(IBridge.runtimeAdministrator.selector, "runtimeAdministrator()");
        _assertSelector(IBridge.baseAdminTimelock.selector, "baseAdminTimelock()");
        _assertSelector(IBridge.serviceFee.selector, "serviceFee()");
        _assertSelector(IBridge.MAX_SERVICE_FEE.selector, "MAX_SERVICE_FEE()");
        _assertSelector(IBridge.perDepositLimit.selector, "perDepositLimit()");
        _assertSelector(IBridge.mintWindowLimit.selector, "mintWindowLimit()");
        _assertSelector(IBridge.mintWindowDuration.selector, "mintWindowDuration()");
        _assertSelector(IBridge.mintWindowStartedAt.selector, "mintWindowStartedAt()");
        _assertSelector(IBridge.mintedInWindow.selector, "mintedInWindow()");
        _assertSelector(IBridge.depositMintsPaused.selector, "depositMintsPaused()");
        _assertSelector(IBridge.withdrawalsPaused.selector, "withdrawalsPaused()");
        _assertSelector(IBridge.nextWithdrawalId.selector, "nextWithdrawalId()");
        _assertSelector(IBridge.approvedTimelockRuntimeCodeHash.selector, "approvedTimelockRuntimeCodeHash()");
        _assertSelector(IBridge.isDepositProcessed.selector, "isDepositProcessed(bytes32)");
        _assertSelector(IBridge.getWithdrawal.selector, "getWithdrawal(uint256)");
    }

    function testBridgeAdministrationSelectors() public pure {
        _assertSelector(IBridge.pauseDepositMints.selector, "pauseDepositMints()");
        _assertSelector(IBridge.pauseWithdrawals.selector, "pauseWithdrawals()");
        _assertSelector(IBridge.unpauseDepositMints.selector, "unpauseDepositMints()");
        _assertSelector(IBridge.unpauseWithdrawals.selector, "unpauseWithdrawals()");
        _assertSelector(IBridge.setServiceFee.selector, "setServiceFee(uint256)");
        _assertSelector(IBridge.rotateBridgeSigner.selector, "rotateBridgeSigner(address)");
        _assertSelector(IBridge.rotateRuntimeAdministrator.selector, "rotateRuntimeAdministrator(address)");
        _assertSelector(IBridge.rotateBaseAdminTimelock.selector, "rotateBaseAdminTimelock(address)");
    }

    function testErrorSelectors() public pure {
        _assertSelector(IBSNS.OnlyBridge.selector, "OnlyBridge(address)");
        _assertSelector(IBSNS.AuthorizationNotYetValid.selector, "AuthorizationNotYetValid(uint256,uint256)");
        _assertSelector(IBSNS.AuthorizationExpired.selector, "AuthorizationExpired(uint256,uint256)");
        _assertSelector(IBSNS.AuthorizationAlreadyUsed.selector, "AuthorizationAlreadyUsed(address,bytes32)");
        _assertSelector(IBSNS.InvalidAuthorizationSigner.selector, "InvalidAuthorizationSigner(address,address)");
        _assertSelector(IBSNS.CallerMustBeRecipient.selector, "CallerMustBeRecipient(address,address)");
        _assertSelector(IBridge.ZeroAddress.selector, "ZeroAddress()");
        _assertSelector(IBridge.RoleAddressesMustDiffer.selector, "RoleAddressesMustDiffer()");
        _assertSelector(IBridge.InvalidAmount.selector, "InvalidAmount(uint256)");
        _assertSelector(IBridge.InvalidPrincipal.selector, "InvalidPrincipal(bytes)");
        _assertSelector(IBridge.InvalidServiceFee.selector, "InvalidServiceFee(uint256,uint256)");
        _assertSelector(IBridge.ServiceFeeExceedsUserMaximum.selector, "ServiceFeeExceedsUserMaximum(uint256,uint256)");
        _assertSelector(IBridge.DepositAlreadyProcessed.selector, "DepositAlreadyProcessed(bytes32)");
        _assertSelector(IBridge.DepositMintLimitExceeded.selector, "DepositMintLimitExceeded(uint256,uint256)");
        _assertSelector(IBridge.MintWindowLimitExceeded.selector, "MintWindowLimitExceeded(uint256,uint256)");
        _assertSelector(IBridge.DepositMintsArePaused.selector, "DepositMintsArePaused()");
        _assertSelector(IBridge.WithdrawalsArePaused.selector, "WithdrawalsArePaused()");
        _assertSelector(IBridge.UnauthorizedBridgeSigner.selector, "UnauthorizedBridgeSigner(address)");
        _assertSelector(IBridge.UnauthorizedRuntimeAdministrator.selector, "UnauthorizedRuntimeAdministrator(address)");
        _assertSelector(IBridge.UnauthorizedBaseAdmin.selector, "UnauthorizedBaseAdmin(address)");
        _assertSelector(IBridge.TimelockCandidateHasNoCode.selector, "TimelockCandidateHasNoCode(address)");
        _assertSelector(
            IBridge.TimelockCandidateCodeHashMismatch.selector,
            "TimelockCandidateCodeHashMismatch(address,bytes32,bytes32)"
        );
        _assertSelector(
            IBridge.TimelockCandidateIntrospectionFailed.selector, "TimelockCandidateIntrospectionFailed(address)"
        );
        _assertSelector(
            IBridge.TimelockCandidateDelayTooShort.selector, "TimelockCandidateDelayTooShort(address,uint256,uint256)"
        );
        _assertSelector(
            IBridge.TimelockCandidateMissingSelfAdmin.selector, "TimelockCandidateMissingSelfAdmin(address)"
        );
    }

    function testTimelockErrorSelectors() public pure {
        _assertSelector(BridgeTimelockController.RoleSetFrozen.selector, "RoleSetFrozen(bytes32,address)");
    }

    function testEventTopics() public pure {
        _assertTopic(IBSNS.AuthorizationUsed.selector, "AuthorizationUsed(address,bytes32)");
        _assertTopic(IBSNS.AuthorizationCanceled.selector, "AuthorizationCanceled(address,bytes32)");
        _assertTopic(IBridge.DepositMinted.selector, "DepositMinted(bytes32,address,uint256,uint256,uint256)");
        _assertTopic(
            IBridge.WithdrawalCommitted.selector,
            "WithdrawalCommitted(uint256,address,uint256,uint256,uint256,uint256,bytes,bytes32)"
        );
        _assertTopic(IBridge.ServiceFeeChanged.selector, "ServiceFeeChanged(address,uint256,uint256)");
        _assertTopic(IBridge.DepositMintsPaused.selector, "DepositMintsPaused(address)");
        _assertTopic(IBridge.DepositMintsUnpaused.selector, "DepositMintsUnpaused(address)");
        _assertTopic(IBridge.WithdrawalsPaused.selector, "WithdrawalsPaused(address)");
        _assertTopic(IBridge.WithdrawalsUnpaused.selector, "WithdrawalsUnpaused(address)");
        _assertTopic(IBridge.BridgeSignerChanged.selector, "BridgeSignerChanged(address,address)");
        _assertTopic(IBridge.RuntimeAdministratorChanged.selector, "RuntimeAdministratorChanged(address,address)");
        _assertTopic(IBridge.BaseAdminTimelockChanged.selector, "BaseAdminTimelockChanged(address,address)");
    }

    function testEIP3009TypeHashes() public pure {
        assert(
            keccak256(
                "TransferWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)"
            ) == 0x7c7c6cdb67a18743f49ec6fa9b35f50d52ed05cbed4cc592e13b44501c1a2267
        );
        assert(
            keccak256(
                "ReceiveWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)"
            ) == 0xd099cc98ef71107a616c4f0f941f04c322d8e254fe26b3c6668db87aae413de8
        );
        assert(
            keccak256("CancelAuthorization(address authorizer,bytes32 nonce)")
                == 0x158b0a9edf7a828aad02f63cd515c68ef2f50ba807396f6d12842833a1597429
        );
    }

    function testEnumOrdinalsAndStructOrder() public pure {
        assert(uint8(IBridge.WithdrawalStatus.None) == 0);
        assert(uint8(IBridge.WithdrawalStatus.Committed) == 1);
        _assertSelector(InterfaceTupleFixture.deposit.selector, "deposit((bytes32,address,uint256,uint256,uint256))");
        _assertSelector(
            InterfaceTupleFixture.withdrawal.selector,
            "withdrawal((address,uint256,uint256,uint256,uint256,bytes,bytes32,uint8))"
        );
    }

    function testConstructorArgumentOrderFixture() public {
        BridgeConstructorFixture fixture = new BridgeConstructorFixture(
            "kinic",
            "KINIC",
            8,
            address(0x11),
            address(0x22),
            address(0x33),
            bytes32(uint256(0x44)),
            100,
            200,
            1 hours,
            10,
            1
        );
        bytes32 expected = keccak256(
            abi.encode(
                "kinic",
                "KINIC",
                uint8(8),
                address(0x11),
                address(0x22),
                address(0x33),
                bytes32(uint256(0x44)),
                uint256(100),
                uint256(200),
                uint64(1 hours),
                uint256(10),
                uint256(1)
            )
        );
        assert(fixture.argumentsHash() == expected);
    }

    function _assertSelector(bytes4 actual, string memory signature) private pure {
        assert(actual == bytes4(keccak256(bytes(signature))));
    }

    function _assertTopic(bytes32 actual, string memory signature) private pure {
        assert(actual == keccak256(bytes(signature)));
    }
}
