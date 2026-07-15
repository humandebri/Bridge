// contracts/test: verify Bridge-only supply control and the EIP-3009 authorization state machine.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {BSNS} from "../src/BSNS.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {TestBase} from "./TestBase.sol";
import {ECDSA} from "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";

contract BSNSTest is TestBase {
    bytes32 private constant EIP712_DOMAIN_TYPEHASH =
        keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");
    bytes32 private constant TRANSFER_TYPEHASH = keccak256(
        "TransferWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)"
    );
    bytes32 private constant RECEIVE_TYPEHASH = keccak256(
        "ReceiveWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)"
    );
    bytes32 private constant CANCEL_TYPEHASH = keccak256("CancelAuthorization(address authorizer,bytes32 nonce)");
    uint256 private constant AUTHORIZER_KEY = 0xA11CE;
    uint256 private constant OTHER_KEY = 0xB0B;
    uint256 private constant SECP256K1_ORDER = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141;

    event AuthorizationUsed(address indexed authorizer, bytes32 indexed nonce);
    event AuthorizationCanceled(address indexed authorizer, bytes32 indexed nonce);

    BSNS private token;
    address private authorizer;
    address private recipient;

    function setUp() public {
        vm.warp(1_000_000);
        token = new BSNS("kinic", "KINIC", 8, address(this));
        authorizer = vm.addr(AUTHORIZER_KEY);
        recipient = address(0xCAFE);
    }

    function testMetadataAndEIP712Domain() public view {
        assert(_sameString(token.name(), "kinic"));
        assert(_sameString(token.symbol(), "KINIC"));
        assert(token.decimals() == 8);
        assert(_sameString(token.version(), "1"));
        assert(token.bridge() == address(this));

        (
            bytes1 fields,
            string memory domainName,
            string memory domainVersion,
            uint256 chainId,
            address verifyingContract,
            bytes32 salt,
            uint256[] memory extensions
        ) = token.eip712Domain();
        assert(fields == hex"0f");
        assert(_sameString(domainName, "kinic"));
        assert(_sameString(domainVersion, "1"));
        assert(chainId == block.chainid);
        assert(verifyingContract == address(token));
        assert(salt == bytes32(0));
        assert(extensions.length == 0);
    }

    function testOnlyBridgeCanMintAndBurn() public {
        BSNS restricted = new BSNS("kinic", "KINIC", 8, address(0xBEEF));
        vm.expectRevert(abi.encodeWithSelector(IBSNS.OnlyBridge.selector, address(this)));
        restricted.bridgeMint(authorizer, 100);

        vm.prank(address(0xBEEF));
        restricted.bridgeMint(authorizer, 100);
        assert(restricted.balanceOf(authorizer) == 100);

        vm.prank(authorizer);
        assert(restricted.transfer(address(0xBEEF), 40));
        vm.prank(address(0xBEEF));
        restricted.bridgeBurn(40);
        assert(restricted.balanceOf(authorizer) == 60);
        assert(restricted.totalSupply() == 60);
    }

    function testStandardERC20AllowanceRemainsAvailable() public {
        token.bridgeMint(authorizer, 100);
        vm.prank(authorizer);
        token.approve(address(this), 70);
        assert(token.transferFrom(authorizer, recipient, 60));
        assert(token.balanceOf(recipient) == 60);
        assert(token.allowance(authorizer, address(this)) == 10);
    }

    function testTransferWithAuthorizationAllowsRelayer() public {
        token.bridgeMint(authorizer, 100);
        bytes32 nonce = keccak256("transfer");
        (uint8 v, bytes32 r, bytes32 s) = _signTransfer(
            AUTHORIZER_KEY,
            TRANSFER_TYPEHASH,
            address(token),
            authorizer,
            recipient,
            60,
            block.timestamp - 1,
            block.timestamp + 1,
            nonce
        );

        vm.expectEmit(true, true, false, true, address(token));
        emit AuthorizationUsed(authorizer, nonce);
        token.transferWithAuthorization(
            authorizer, recipient, 60, block.timestamp - 1, block.timestamp + 1, nonce, v, r, s
        );

        assert(token.balanceOf(authorizer) == 40);
        assert(token.balanceOf(recipient) == 60);
        assert(token.authorizationState(authorizer, nonce));
    }

    function testReceiveWithAuthorizationRequiresRecipientCaller() public {
        token.bridgeMint(authorizer, 100);
        bytes32 nonce = keccak256("receive");
        uint256 validAfter = block.timestamp - 1;
        uint256 validBefore = block.timestamp + 1;
        (uint8 v, bytes32 r, bytes32 s) = _signTransfer(
            AUTHORIZER_KEY, RECEIVE_TYPEHASH, address(token), authorizer, recipient, 50, validAfter, validBefore, nonce
        );

        vm.expectRevert(abi.encodeWithSelector(IBSNS.CallerMustBeRecipient.selector, address(this), recipient));
        token.receiveWithAuthorization(authorizer, recipient, 50, validAfter, validBefore, nonce, v, r, s);
        assert(!token.authorizationState(authorizer, nonce));

        vm.prank(recipient);
        token.receiveWithAuthorization(authorizer, recipient, 50, validAfter, validBefore, nonce, v, r, s);
        assert(token.balanceOf(recipient) == 50);
    }

    function testAuthorizationUsesStrictTimeBounds() public {
        bytes32 earlyNonce = keccak256("early");
        (uint8 earlyV, bytes32 earlyR, bytes32 earlyS) = _signTransfer(
            AUTHORIZER_KEY,
            TRANSFER_TYPEHASH,
            address(token),
            authorizer,
            recipient,
            1,
            block.timestamp,
            block.timestamp + 10,
            earlyNonce
        );
        vm.expectRevert(
            abi.encodeWithSelector(IBSNS.AuthorizationNotYetValid.selector, block.timestamp, block.timestamp)
        );
        token.transferWithAuthorization(
            authorizer, recipient, 1, block.timestamp, block.timestamp + 10, earlyNonce, earlyV, earlyR, earlyS
        );

        bytes32 expiredNonce = keccak256("expired");
        (uint8 expiredV, bytes32 expiredR, bytes32 expiredS) = _signTransfer(
            AUTHORIZER_KEY,
            TRANSFER_TYPEHASH,
            address(token),
            authorizer,
            recipient,
            1,
            block.timestamp - 10,
            block.timestamp,
            expiredNonce
        );
        vm.expectRevert(abi.encodeWithSelector(IBSNS.AuthorizationExpired.selector, block.timestamp, block.timestamp));
        token.transferWithAuthorization(
            authorizer, recipient, 1, block.timestamp - 10, block.timestamp, expiredNonce, expiredV, expiredR, expiredS
        );
    }

    function testUsedNonceCannotBeCanceled() public {
        token.bridgeMint(authorizer, 10);
        bytes32 nonce = keccak256("shared-used");
        (uint8 transferV, bytes32 transferR, bytes32 transferS) = _signTransfer(
            AUTHORIZER_KEY,
            TRANSFER_TYPEHASH,
            address(token),
            authorizer,
            recipient,
            1,
            block.timestamp - 1,
            block.timestamp + 1,
            nonce
        );
        token.transferWithAuthorization(
            authorizer, recipient, 1, block.timestamp - 1, block.timestamp + 1, nonce, transferV, transferR, transferS
        );

        (uint8 cancelV, bytes32 cancelR, bytes32 cancelS) =
            _signCancel(AUTHORIZER_KEY, address(token), authorizer, nonce);
        vm.expectRevert(abi.encodeWithSelector(IBSNS.AuthorizationAlreadyUsed.selector, authorizer, nonce));
        token.cancelAuthorization(authorizer, nonce, cancelV, cancelR, cancelS);
    }

    function testCanceledNonceCannotBeTransferred() public {
        bytes32 nonce = keccak256("shared-canceled");
        (uint8 cancelV, bytes32 cancelR, bytes32 cancelS) =
            _signCancel(AUTHORIZER_KEY, address(token), authorizer, nonce);
        vm.expectEmit(true, true, false, true, address(token));
        emit AuthorizationCanceled(authorizer, nonce);
        token.cancelAuthorization(authorizer, nonce, cancelV, cancelR, cancelS);

        (uint8 transferV, bytes32 transferR, bytes32 transferS) = _signTransfer(
            AUTHORIZER_KEY,
            TRANSFER_TYPEHASH,
            address(token),
            authorizer,
            recipient,
            1,
            block.timestamp - 1,
            block.timestamp + 1,
            nonce
        );
        vm.expectRevert(abi.encodeWithSelector(IBSNS.AuthorizationAlreadyUsed.selector, authorizer, nonce));
        token.transferWithAuthorization(
            authorizer, recipient, 1, block.timestamp - 1, block.timestamp + 1, nonce, transferV, transferR, transferS
        );
    }

    function testRejectsWrongSignerAndWrongDomain() public {
        bytes32 wrongSignerNonce = keccak256("wrong-signer");
        address wrongSigner = vm.addr(OTHER_KEY);
        (uint8 v, bytes32 r, bytes32 s) = _signTransfer(
            OTHER_KEY,
            TRANSFER_TYPEHASH,
            address(token),
            authorizer,
            recipient,
            1,
            block.timestamp - 1,
            block.timestamp + 1,
            wrongSignerNonce
        );
        vm.expectRevert(abi.encodeWithSelector(IBSNS.InvalidAuthorizationSigner.selector, wrongSigner, authorizer));
        token.transferWithAuthorization(
            authorizer, recipient, 1, block.timestamp - 1, block.timestamp + 1, wrongSignerNonce, v, r, s
        );

        bytes32 wrongDomainNonce = keccak256("wrong-domain");
        (v, r, s) = _signTransfer(
            AUTHORIZER_KEY,
            TRANSFER_TYPEHASH,
            address(0xDEAD),
            authorizer,
            recipient,
            1,
            block.timestamp - 1,
            block.timestamp + 1,
            wrongDomainNonce
        );
        vm.expectPartialRevert(IBSNS.InvalidAuthorizationSigner.selector);
        token.transferWithAuthorization(
            authorizer, recipient, 1, block.timestamp - 1, block.timestamp + 1, wrongDomainNonce, v, r, s
        );

        bytes32 wrongChainNonce = keccak256("wrong-chain");
        bytes32 wrongChainStructHash = keccak256(
            abi.encode(
                TRANSFER_TYPEHASH, authorizer, recipient, 1, block.timestamp - 1, block.timestamp + 1, wrongChainNonce
            )
        );
        (v, r, s) =
            vm.sign(AUTHORIZER_KEY, _typedDataHashForChain(address(token), block.chainid + 1, wrongChainStructHash));
        vm.expectPartialRevert(IBSNS.InvalidAuthorizationSigner.selector);
        token.transferWithAuthorization(
            authorizer, recipient, 1, block.timestamp - 1, block.timestamp + 1, wrongChainNonce, v, r, s
        );
    }

    function testRejectsHighSSignature() public {
        bytes32 nonce = keccak256("high-s");
        (uint8 v, bytes32 r, bytes32 s) = _signTransfer(
            AUTHORIZER_KEY,
            TRANSFER_TYPEHASH,
            address(token),
            authorizer,
            recipient,
            1,
            block.timestamp - 1,
            block.timestamp + 1,
            nonce
        );
        bytes32 highS = bytes32(SECP256K1_ORDER - uint256(s));
        uint8 flippedV = v == 27 ? 28 : 27;
        vm.expectRevert(abi.encodeWithSelector(ECDSA.ECDSAInvalidSignatureS.selector, highS));
        token.transferWithAuthorization(
            authorizer, recipient, 1, block.timestamp - 1, block.timestamp + 1, nonce, flippedV, r, highS
        );
    }

    function testRejectsInvalidSignature() public {
        bytes32 nonce = keccak256("invalid-signature");
        vm.expectRevert(ECDSA.ECDSAInvalidSignature.selector);
        token.transferWithAuthorization(
            authorizer, recipient, 1, block.timestamp - 1, block.timestamp + 1, nonce, 0, bytes32(0), bytes32(0)
        );
        assert(!token.authorizationState(authorizer, nonce));
    }

    function testTransferFailureDoesNotConsumeAuthorization() public {
        bytes32 nonce = keccak256("insufficient-balance");
        (uint8 v, bytes32 r, bytes32 s) = _signTransfer(
            AUTHORIZER_KEY,
            TRANSFER_TYPEHASH,
            address(token),
            authorizer,
            recipient,
            10,
            block.timestamp - 1,
            block.timestamp + 1,
            nonce
        );
        vm.expectRevert(
            abi.encodeWithSelector(
                bytes4(keccak256("ERC20InsufficientBalance(address,uint256,uint256)")), authorizer, 0, 10
            )
        );
        token.transferWithAuthorization(
            authorizer, recipient, 10, block.timestamp - 1, block.timestamp + 1, nonce, v, r, s
        );
        assert(!token.authorizationState(authorizer, nonce));

        token.bridgeMint(authorizer, 10);
        token.transferWithAuthorization(
            authorizer, recipient, 10, block.timestamp - 1, block.timestamp + 1, nonce, v, r, s
        );
        assert(token.balanceOf(recipient) == 10);
    }

    function testFuzzAuthorizationNonceNamespace(uint256 nonceSeed) public {
        token.bridgeMint(authorizer, 1);
        bytes32 nonce = bytes32(nonceSeed);
        uint256 validAfter = block.timestamp - 1;
        uint256 validBefore = block.timestamp + 2;
        (uint8 transferV, bytes32 transferR, bytes32 transferS) = _signTransfer(
            AUTHORIZER_KEY, TRANSFER_TYPEHASH, address(token), authorizer, recipient, 1, validAfter, validBefore, nonce
        );

        token.transferWithAuthorization(
            authorizer, recipient, 1, validAfter, validBefore, nonce, transferV, transferR, transferS
        );
        assert(token.authorizationState(authorizer, nonce));

        (uint8 cancelV, bytes32 cancelR, bytes32 cancelS) =
            _signCancel(AUTHORIZER_KEY, address(token), authorizer, nonce);
        vm.expectRevert(abi.encodeWithSelector(IBSNS.AuthorizationAlreadyUsed.selector, authorizer, nonce));
        token.cancelAuthorization(authorizer, nonce, cancelV, cancelR, cancelS);
    }

    function _signTransfer(
        uint256 privateKey,
        bytes32 typeHash,
        address verifyingContract,
        address from,
        address to,
        uint256 value,
        uint256 validAfter,
        uint256 validBefore,
        bytes32 nonce
    ) private returns (uint8 v, bytes32 r, bytes32 s) {
        bytes32 structHash = keccak256(abi.encode(typeHash, from, to, value, validAfter, validBefore, nonce));
        return vm.sign(privateKey, _typedDataHash(verifyingContract, structHash));
    }

    function _signCancel(uint256 privateKey, address verifyingContract, address from, bytes32 nonce)
        private
        returns (uint8 v, bytes32 r, bytes32 s)
    {
        bytes32 structHash = keccak256(abi.encode(CANCEL_TYPEHASH, from, nonce));
        return vm.sign(privateKey, _typedDataHash(verifyingContract, structHash));
    }

    function _typedDataHash(address verifyingContract, bytes32 structHash) private view returns (bytes32) {
        return _typedDataHashForChain(verifyingContract, block.chainid, structHash);
    }

    function _typedDataHashForChain(address verifyingContract, uint256 chainId, bytes32 structHash)
        private
        pure
        returns (bytes32)
    {
        bytes32 domainSeparator = keccak256(
            abi.encode(
                EIP712_DOMAIN_TYPEHASH, keccak256(bytes("kinic")), keccak256(bytes("1")), chainId, verifyingContract
            )
        );
        return keccak256(abi.encodePacked(hex"1901", domainSeparator, structHash));
    }
}
