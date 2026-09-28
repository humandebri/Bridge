// contracts/test: verify asset isolation in the shared Base Bridge.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IMultiTokenBridge} from "../src/interfaces/IMultiTokenBridge.sol";
import {MultiTokenBridge} from "../src/MultiTokenBridge.sol";
import {TestBase} from "./TestBase.sol";

contract MultiTokenBridgeTest is TestBase {
    uint256 private constant SIGNER_KEY = 0xA11CE;
    address private signer;
    address private constant RUNTIME_ADMINISTRATOR = address(0x22);
    address private timelock;
    address private constant USER = address(0x44);
    bytes32 private constant ASSET_A = keccak256("asset-a");
    bytes32 private constant ASSET_B = keccak256("asset-b");
    bytes32 private constant MULTI_MINT_TYPEHASH = keccak256(
        "MintAuthorization(bytes32 assetId,bytes32 depositId,address recipient,uint256 grossAmount,uint256 maxServiceFee,uint256 chargedServiceFee,uint256 deadline,uint256 globalEpoch,uint256 assetEpoch)"
    );

    MultiTokenBridge private bridge;
    IBSNS private tokenA;
    IBSNS private tokenB;

    function setUp() public {
        signer = vm.addr(SIGNER_KEY);
        timelock = _deployTestTimelock(address(0x33));
        vm.warp(1_000_000);
        bridge = new MultiTokenBridge(signer, RUNTIME_ADMINISTRATOR, timelock, _timelockCodeHash(timelock));
        vm.startPrank(timelock);
        tokenA = IBSNS(bridge.registerAsset(_registration(ASSET_A, "Asset A", "ASSETA", 8)));
        tokenB = IBSNS(bridge.registerAsset(_registration(ASSET_B, "Asset B", "ASSETB", 6)));
        bridge.unpauseAssetDepositMints(ASSET_A);
        bridge.unpauseAssetWithdrawals(ASSET_A);
        bridge.unpauseAssetDepositMints(ASSET_B);
        bridge.unpauseAssetWithdrawals(ASSET_B);
        bridge.unpauseAllDepositMints();
        bridge.unpauseAllWithdrawals();
        vm.stopPrank();
    }

    function testRegistrationCreatesDistinctBridgeBoundTokens() public view {
        assert(address(tokenA) != address(tokenB));
        assert(tokenA.bridge() == address(bridge));
        assert(tokenB.bridge() == address(bridge));
        assert(tokenA.decimals() == 8);
        assert(tokenB.decimals() == 6);
        assert(bridge.tokenForAsset(ASSET_A) == address(tokenA));
        assert(bridge.tokenForAsset(ASSET_B) == address(tokenB));
    }

    function testRegistrationRejectsDuplicateAndUnknownAssets() public {
        vm.prank(timelock);
        vm.expectRevert(abi.encodeWithSelector(IMultiTokenBridge.AssetAlreadyRegistered.selector, ASSET_A));
        bridge.registerAsset(_registration(ASSET_A, "Again", "AGAIN", 8));

        bytes32 unknown = keccak256("unknown");
        vm.expectRevert(abi.encodeWithSelector(IMultiTokenBridge.UnknownAsset.selector, unknown));
        bridge.tokenForAsset(unknown);
    }

    function testOnlyTimelockCanRegisterOrUnpauseAssets() public {
        bytes32 asset = keccak256("unauthorized");
        vm.expectRevert(abi.encodeWithSelector(IMultiTokenBridge.UnauthorizedBaseAdmin.selector, address(this)));
        bridge.registerAsset(_registration(asset, "Unauthorized", "NO", 8));

        vm.prank(RUNTIME_ADMINISTRATOR);
        vm.expectRevert(abi.encodeWithSelector(IMultiTokenBridge.UnauthorizedBaseAdmin.selector, RUNTIME_ADMINISTRATOR));
        bridge.unpauseAssetWithdrawals(ASSET_A);
    }

    function testMintIsBoundToAssetAndDoesNotChangeOtherSupply() public {
        IMultiTokenBridge.MintAuthorization memory authorization = _authorization(ASSET_A, keccak256("deposit"));
        bridge.mintDepositWithAuthorization(authorization, _signature(authorization));

        assert(tokenA.balanceOf(USER) == 100);
        assert(tokenB.balanceOf(USER) == 0);
        assert(bridge.isDepositProcessed(ASSET_A, authorization.depositId));
        assert(!bridge.isDepositProcessed(ASSET_B, authorization.depositId));
    }

    function testSignatureCannotBeReplayedForAnotherAsset() public {
        IMultiTokenBridge.MintAuthorization memory authorization = _authorization(ASSET_A, keccak256("deposit"));
        bytes memory signature = _signature(authorization);
        authorization.assetId = ASSET_B;
        vm.expectRevert(IMultiTokenBridge.InvalidMintAuthorizationSignature.selector);
        bridge.mintDepositWithAuthorization(authorization, signature);
    }

    function testAssetPauseInvalidatesOnlyThatAssetAndCanResumeThroughTimelock() public {
        uint256 globalEpoch = bridge.globalEpoch();
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.pauseAssetDepositMints(ASSET_A);

        IMultiTokenBridge.MintAuthorization memory assetA = _authorization(ASSET_A, keccak256("paused"));
        vm.expectRevert(abi.encodeWithSelector(IMultiTokenBridge.DepositMintsArePaused.selector, ASSET_A));
        bridge.mintDepositWithAuthorization(assetA, _signature(assetA));

        IMultiTokenBridge.MintAuthorization memory assetB = _authorization(ASSET_B, keccak256("active"));
        bridge.mintDepositWithAuthorization(assetB, _signature(assetB));
        assert(tokenB.balanceOf(USER) == 100);

        vm.prank(timelock);
        bridge.unpauseAssetDepositMints(ASSET_A);
        assert(bridge.globalEpoch() == globalEpoch);
        IMultiTokenBridge.MintAuthorization memory resumed = _authorization(ASSET_A, keccak256("resumed"));
        bridge.mintDepositWithAuthorization(resumed, _signature(resumed));
        assert(tokenA.balanceOf(USER) == 100);
    }

    function testWithdrawalBurnsOnlySelectedAssetAndRecordsBinding() public {
        IMultiTokenBridge.MintAuthorization memory authorization = _authorization(ASSET_A, keccak256("withdraw"));
        bridge.mintDepositWithAuthorization(authorization, _signature(authorization));
        vm.startPrank(USER);
        tokenA.approve(address(bridge), 100);
        uint256 withdrawalId = bridge.createWithdrawal(ASSET_A, 100, 10, hex"01", bytes32(0));
        vm.stopPrank();

        IMultiTokenBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
        assert(withdrawal.assetId == ASSET_A);
        assert(withdrawal.requester == USER);
        assert(withdrawal.amount == 100);
        assert(withdrawal.amountOut == 90);
        assert(tokenA.totalSupply() == 0);
        assert(tokenB.totalSupply() == 0);
    }

    function testWithdrawalFailureRollsBackRecordAndIdentifier() public {
        uint256 nextWithdrawalId = bridge.nextWithdrawalId();
        vm.prank(USER);
        vm.expectRevert(
            abi.encodeWithSelector(
                bytes4(keccak256("ERC20InsufficientAllowance(address,uint256,uint256)")), address(bridge), 0, 100
            )
        );
        bridge.createWithdrawal(ASSET_A, 100, 10, hex"01", bytes32(0));

        assert(bridge.nextWithdrawalId() == nextWithdrawalId);
        assert(bridge.getWithdrawal(nextWithdrawalId).status == IMultiTokenBridge.WithdrawalStatus.None);
    }

    function testFeeAndWindowAccountingAreAssetLocal() public {
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setAssetServiceFee(ASSET_A, 20);
        IMultiTokenBridge.AssetSnapshot memory a = bridge.assetSnapshot(ASSET_A);
        IMultiTokenBridge.AssetSnapshot memory b = bridge.assetSnapshot(ASSET_B);
        assert(a.serviceFee == 20);
        assert(b.serviceFee == 10);

        IMultiTokenBridge.MintAuthorization memory authorization = _authorization(ASSET_A, keccak256("window"));
        bridge.mintDepositWithAuthorization(authorization, _signature(authorization));
        a = bridge.assetSnapshot(ASSET_A);
        b = bridge.assetSnapshot(ASSET_B);
        assert(a.mintedInWindow == 100);
        assert(b.mintedInWindow == 0);
    }

    function testGlobalDepositPauseInvalidatesAllAuthorizations() public {
        IMultiTokenBridge.MintAuthorization memory authorization = _authorization(ASSET_A, keccak256("global"));
        uint256 previousEpoch = bridge.globalEpoch();

        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.pauseAllDepositMints();

        assert(bridge.globalEpoch() == previousEpoch + 1);
        vm.expectRevert(abi.encodeWithSelector(IMultiTokenBridge.DepositMintsArePaused.selector, ASSET_A));
        bridge.mintDepositWithAuthorization(authorization, _signature(authorization));
    }

    function testMintRejectsDeadlineBeyondFifteenMinutes() public {
        IMultiTokenBridge.MintAuthorization memory authorization = _authorization(ASSET_A, keccak256("deadline"));
        authorization.deadline = block.timestamp + 15 minutes + 1;
        vm.expectRevert(
            abi.encodeWithSelector(
                IMultiTokenBridge.MintAuthorizationDeadlineTooFar.selector,
                authorization.deadline,
                block.timestamp + 15 minutes
            )
        );
        bridge.mintDepositWithAuthorization(authorization, _signature(authorization));
    }

    function _registration(bytes32 assetId, string memory name, string memory symbol, uint8 decimals)
        private
        pure
        returns (IMultiTokenBridge.AssetRegistration memory)
    {
        return IMultiTokenBridge.AssetRegistration({
            assetId: assetId,
            tokenName: name,
            tokenSymbol: symbol,
            tokenDecimals: decimals,
            perDepositLimit: 1_000,
            mintWindowLimit: 2_000,
            mintWindowDuration: 1 hours,
            minServiceFee: 1,
            maxServiceFee: 100,
            initialServiceFee: 10
        });
    }

    function _authorization(bytes32 assetId, bytes32 depositId)
        private
        view
        returns (IMultiTokenBridge.MintAuthorization memory)
    {
        return IMultiTokenBridge.MintAuthorization({
            assetId: assetId,
            depositId: depositId,
            recipient: USER,
            grossAmount: 110,
            maxServiceFee: 10,
            chargedServiceFee: 10,
            deadline: block.timestamp + 15 minutes,
            globalEpoch: bridge.globalEpoch(),
            assetEpoch: bridge.assetSnapshot(assetId).assetEpoch
        });
    }

    function _signature(IMultiTokenBridge.MintAuthorization memory authorization) private returns (bytes memory) {
        bytes32 domainSeparator = keccak256(
            abi.encode(
                MINT_EIP712_DOMAIN_TYPEHASH,
                keccak256("IC Base Multi-Token Bridge"),
                keccak256("1"),
                block.chainid,
                address(bridge)
            )
        );
        bytes32 structHash = keccak256(
            abi.encode(
                MULTI_MINT_TYPEHASH,
                authorization.assetId,
                authorization.depositId,
                authorization.recipient,
                authorization.grossAmount,
                authorization.maxServiceFee,
                authorization.chargedServiceFee,
                authorization.deadline,
                authorization.globalEpoch,
                authorization.assetEpoch
            )
        );
        (uint8 v, bytes32 r, bytes32 s) =
            vm.sign(SIGNER_KEY, keccak256(abi.encodePacked(hex"1901", domainSeparator, structHash)));
        return abi.encodePacked(r, s, v);
    }
}
