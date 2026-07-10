// contracts/src/interfaces: define the bSNS ERC-20 and EIP-3009 ABI consumed by Base integrations.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {IERC20Metadata} from "@openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol";
import {IERC5267} from "@openzeppelin/contracts/interfaces/IERC5267.sol";

/// @notice ERC-20 metadata, EIP-3009 authorization transfers, and the immutable Bridge-only supply interface.
interface IBSNS is IERC20Metadata, IERC5267 {
    event AuthorizationUsed(address indexed authorizer, bytes32 indexed nonce);
    event AuthorizationCanceled(address indexed authorizer, bytes32 indexed nonce);

    error OnlyBridge(address caller);
    error AuthorizationNotYetValid(uint256 currentTime, uint256 validAfter);
    error AuthorizationExpired(uint256 currentTime, uint256 validBefore);
    error AuthorizationAlreadyUsed(address authorizer, bytes32 nonce);
    error InvalidAuthorizationSigner(address recoveredSigner, address authorizer);
    error CallerMustBeRecipient(address caller, address recipient);

    function bridge() external view returns (address);

    function bridgeMint(address recipient, uint256 amount) external;

    function bridgeBurn(address account, uint256 amount) external;

    function version() external pure returns (string memory);

    function authorizationState(address authorizer, bytes32 nonce) external view returns (bool);

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
    ) external;

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
    ) external;

    function cancelAuthorization(address authorizer, bytes32 nonce, uint8 v, bytes32 r, bytes32 s) external;
}
