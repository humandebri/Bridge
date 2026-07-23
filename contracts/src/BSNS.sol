// contracts/src: implement the non-upgradeable bSNS token and signed EIP-3009 transfers on Base.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {IBSNS} from "./interfaces/IBSNS.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20Metadata} from "@openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol";
import {ECDSA} from "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import {EIP712} from "@openzeppelin/contracts/utils/cryptography/EIP712.sol";

contract BSNS is IBSNS, ERC20, EIP712 {
    bytes32 private constant TRANSFER_WITH_AUTHORIZATION_TYPEHASH =
        0x7c7c6cdb67a18743f49ec6fa9b35f50d52ed05cbed4cc592e13b44501c1a2267;
    bytes32 private constant RECEIVE_WITH_AUTHORIZATION_TYPEHASH =
        0xd099cc98ef71107a616c4f0f941f04c322d8e254fe26b3c6668db87aae413de8;
    bytes32 private constant CANCEL_AUTHORIZATION_TYPEHASH =
        0x158b0a9edf7a828aad02f63cd515c68ef2f50ba807396f6d12842833a1597429;

    address public immutable override bridge;
    uint8 private immutable _tokenDecimals;
    mapping(address authorizer => mapping(bytes32 nonce => bool used)) private _authorizationStates;

    modifier onlyBridge() {
        if (msg.sender != bridge) {
            revert OnlyBridge(msg.sender);
        }
        _;
    }

    constructor(string memory tokenName, string memory tokenSymbol, uint8 tokenDecimals, address bridgeAddress)
        ERC20(tokenName, tokenSymbol)
        EIP712(tokenName, "1")
    {
        bridge = bridgeAddress;
        _tokenDecimals = tokenDecimals;
    }

    function decimals() public view override(ERC20, IERC20Metadata) returns (uint8) {
        return _tokenDecimals;
    }

    function version() external pure override returns (string memory) {
        return "1";
    }

    function bridgeMint(address recipient, uint256 amount) external override onlyBridge {
        _mint(recipient, amount);
    }

    function bridgeBurn(uint256 amount) external override onlyBridge {
        _burn(msg.sender, amount);
    }

    function authorizationState(address authorizer, bytes32 nonce) external view override returns (bool) {
        return _authorizationStates[authorizer][nonce];
    }

    function transferWithAuthorization(
        address from,
        address to,
        uint256 value,
        uint256 validAfter,
        uint256 validBefore,
        bytes32 nonce,
        uint8 v,
        bytes32 r,
        bytes32 s
    ) external override {
        _transferWithAuthorization(
            TRANSFER_WITH_AUTHORIZATION_TYPEHASH, from, to, value, validAfter, validBefore, nonce, v, r, s
        );
    }

    function receiveWithAuthorization(
        address from,
        address to,
        uint256 value,
        uint256 validAfter,
        uint256 validBefore,
        bytes32 nonce,
        uint8 v,
        bytes32 r,
        bytes32 s
    ) external override {
        if (msg.sender != to) {
            revert CallerMustBeRecipient(msg.sender, to);
        }
        _transferWithAuthorization(
            RECEIVE_WITH_AUTHORIZATION_TYPEHASH, from, to, value, validAfter, validBefore, nonce, v, r, s
        );
    }

    function cancelAuthorization(address authorizer, bytes32 nonce, uint8 v, bytes32 r, bytes32 s) external override {
        _requireUnusedAuthorization(authorizer, nonce);
        bytes32 structHash = keccak256(abi.encode(CANCEL_AUTHORIZATION_TYPEHASH, authorizer, nonce));
        address recoveredSigner = ECDSA.recover(_hashTypedDataV4(structHash), v, r, s);
        if (recoveredSigner != authorizer) {
            revert InvalidAuthorizationSigner(recoveredSigner, authorizer);
        }

        _authorizationStates[authorizer][nonce] = true;
        emit AuthorizationCanceled(authorizer, nonce);
    }

    function _transferWithAuthorization(
        bytes32 typeHash,
        address from,
        address to,
        uint256 value,
        uint256 validAfter,
        uint256 validBefore,
        bytes32 nonce,
        uint8 v,
        bytes32 r,
        bytes32 s
    ) private {
        // EIP-3009 defines authorization validity against the current block timestamp.
        // forge-lint: disable-next-line(block-timestamp)
        if (block.timestamp <= validAfter) {
            revert AuthorizationNotYetValid(block.timestamp, validAfter);
        }
        // forge-lint: disable-next-line(block-timestamp)
        if (block.timestamp >= validBefore) {
            revert AuthorizationExpired(block.timestamp, validBefore);
        }
        _requireUnusedAuthorization(from, nonce);

        bytes32 structHash = keccak256(abi.encode(typeHash, from, to, value, validAfter, validBefore, nonce));
        address recoveredSigner = ECDSA.recover(_hashTypedDataV4(structHash), v, r, s);
        if (recoveredSigner != from) {
            revert InvalidAuthorizationSigner(recoveredSigner, from);
        }

        // Consume the nonce before moving value; any ERC-20 failure reverts the nonce update atomically.
        _authorizationStates[from][nonce] = true;
        emit AuthorizationUsed(from, nonce);
        _transfer(from, to, value);
    }

    function _requireUnusedAuthorization(address authorizer, bytes32 nonce) private view {
        if (_authorizationStates[authorizer][nonce]) {
            revert AuthorizationAlreadyUsed(authorizer, nonce);
        }
    }
}
