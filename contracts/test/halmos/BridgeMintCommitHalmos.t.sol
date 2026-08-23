// contracts/test/halmos: symbolically verify the production post-authentication Mint commit boundary.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../../src/Bridge.sol";
import {IBSNS} from "../../src/interfaces/IBSNS.sol";
import {IBridge} from "../../src/interfaces/IBridge.sol";
import {MintAuthorizationPolicy} from "../../src/libraries/MintAuthorizationPolicy.sol";
import {TestBase} from "../TestBase.sol";

contract HalmosTimelockCandidate {
    address private constant MEMBER = address(0xBEEF);

    function getMinDelay() external pure returns (uint256) {
        return 24 hours;
    }

    function hasRole(bytes32 role, address account) external view returns (bool) {
        return role == bytes32(0) ? account == address(this) : account == MEMBER;
    }

    function roleMember(bytes32 role) external view returns (address) {
        return role == bytes32(0) ? address(this) : MEMBER;
    }

    function pendingOperationCount() external pure returns (uint256) {
        return 0;
    }
}

contract RevertingMintTarget {
    fallback() external payable {
        revert();
    }
}

contract BridgeMintCommitHarness is Bridge {
    constructor(address timelock)
        Bridge(
            address(0x11),
            address(0x22),
            timelock,
            timelock.codehash,
            type(uint128).max,
            type(uint128).max,
            1 hours,
            type(uint128).max,
            0
        )
    {}

    function exposedCommit(
        IBridge.MintAuthorization calldata authorization,
        bytes32 digest,
        MintAuthorizationPolicy.MintEffects memory effects
    ) external {
        _commitAuthorizedMint(authorization, digest, effects);
    }
}

contract BridgeMintCommitHalmos is TestBase {
    BridgeMintCommitHarness private bridge;
    IBSNS private token;

    function setUp() public {
        HalmosTimelockCandidate timelock = new HalmosTimelockCandidate();
        bridge = new BridgeMintCommitHarness(address(timelock));
        token = bridge.bsns();
    }

    function check_mint_commit_state(
        bytes32 depositId,
        address recipient,
        uint128 mintAmount,
        uint64 windowStartedAtAfter,
        uint128 windowConsumedAfter
    ) public {
        vm.assume(recipient != address(0));
        vm.assume(recipient != address(bridge));
        vm.assume(recipient != address(token));
        vm.assume(mintAmount > 0);

        (IBridge.MintAuthorization memory authorization, MintAuthorizationPolicy.MintEffects memory effects) =
            _commitInput(depositId, recipient, mintAmount, windowStartedAtAfter, windowConsumedAfter);
        bridge.exposedCommit(authorization, bytes32(uint256(1)), effects);

        assert(bridge.isDepositProcessed(depositId));
        assert(bridge.mintWindowStartedAt() == windowStartedAtAfter);
        assert(bridge.mintedInWindow() == windowConsumedAfter);
    }

    function check_mint_commit_supply(bytes32 depositId, address recipient, uint128 mintAmount) public {
        vm.assume(recipient != address(0));
        vm.assume(recipient != address(bridge));
        vm.assume(recipient != address(token));
        vm.assume(mintAmount > 0);

        uint256 balanceBefore = token.balanceOf(recipient);
        uint256 supplyBefore = token.totalSupply();
        (IBridge.MintAuthorization memory authorization, MintAuthorizationPolicy.MintEffects memory effects) =
            _commitInput(depositId, recipient, mintAmount, 1, mintAmount);
        bridge.exposedCommit(authorization, bytes32(uint256(2)), effects);

        assert(token.balanceOf(recipient) == balanceBefore + mintAmount);
        assert(token.totalSupply() == supplyBefore + mintAmount);
    }

    function check_mint_commit_atomicity(bytes32 depositId, address recipient, uint128 mintAmount) public {
        vm.assume(recipient != address(0));
        vm.assume(recipient != address(bridge));
        vm.assume(recipient != address(token));
        vm.assume(mintAmount > 0);

        uint64 startedBefore = bridge.mintWindowStartedAt();
        uint256 consumedBefore = bridge.mintedInWindow();
        RevertingMintTarget reverter = new RevertingMintTarget();
        vm.etch(address(token), address(reverter).code);
        (IBridge.MintAuthorization memory authorization, MintAuthorizationPolicy.MintEffects memory effects) =
            _commitInput(depositId, recipient, mintAmount, 1, mintAmount);
        (bool succeeded,) = address(bridge)
            .call(abi.encodeCall(BridgeMintCommitHarness.exposedCommit, (authorization, bytes32(uint256(3)), effects)));

        assert(!succeeded);
        assert(!bridge.isDepositProcessed(depositId));
        assert(bridge.mintWindowStartedAt() == startedBefore);
        assert(bridge.mintedInWindow() == consumedBefore);
    }

    function _commitInput(
        bytes32 depositId,
        address recipient,
        uint128 mintAmount,
        uint64 windowStartedAtAfter,
        uint128 windowConsumedAfter
    )
        private
        pure
        returns (IBridge.MintAuthorization memory authorization, MintAuthorizationPolicy.MintEffects memory effects)
    {
        authorization.depositId = depositId;
        authorization.recipient = recipient;
        authorization.grossAmount = uint256(mintAmount) + 1;
        authorization.chargedServiceFee = 1;
        effects.processedAfter = true;
        effects.windowStartedAtAfter = windowStartedAtAfter;
        effects.windowConsumedAfter = windowConsumedAfter;
        effects.mintAmount = mintAmount;
        effects.supplyIncrease = mintAmount;
        effects.eventGrossAmount = uint256(mintAmount) + 1;
        effects.eventServiceFee = 1;
        effects.eventMintedAmount = mintAmount;
    }
}
