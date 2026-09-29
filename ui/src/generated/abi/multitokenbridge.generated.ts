/* Generated from contracts/abi/MultiTokenBridge.json. Do not edit. */
export const multiTokenBridgeAbi = [
  {
    "type": "constructor",
    "inputs": [
      {
        "name": "initialBridgeSigner",
        "type": "address",
        "internalType": "address"
      },
      {
        "name": "initialRuntimeAdministrator",
        "type": "address",
        "internalType": "address"
      },
      {
        "name": "initialBaseAdminTimelock",
        "type": "address",
        "internalType": "address"
      },
      {
        "name": "initialApprovedTimelockRuntimeCodeHash",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "approvedTimelockRuntimeCodeHash",
    "inputs": [],
    "outputs": [
      {
        "name": "",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "assetCount",
    "inputs": [],
    "outputs": [
      {
        "name": "",
        "type": "uint256",
        "internalType": "uint256"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "assetSnapshot",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ],
    "outputs": [
      {
        "name": "",
        "type": "tuple",
        "internalType": "struct IMultiTokenBridge.AssetSnapshot",
        "components": [
          {
            "name": "token",
            "type": "address",
            "internalType": "address"
          },
          {
            "name": "assetEpoch",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "serviceFee",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "minServiceFee",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "maxServiceFee",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "perDepositLimit",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "mintWindowLimit",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "mintWindowDuration",
            "type": "uint64",
            "internalType": "uint64"
          },
          {
            "name": "mintWindowStartedAt",
            "type": "uint64",
            "internalType": "uint64"
          },
          {
            "name": "mintedInWindow",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "depositMintsPaused",
            "type": "bool",
            "internalType": "bool"
          },
          {
            "name": "withdrawalsPaused",
            "type": "bool",
            "internalType": "bool"
          }
        ]
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "baseAdminTimelock",
    "inputs": [],
    "outputs": [
      {
        "name": "",
        "type": "address",
        "internalType": "address"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "bridgeSigner",
    "inputs": [],
    "outputs": [
      {
        "name": "",
        "type": "address",
        "internalType": "address"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "createWithdrawal",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      },
      {
        "name": "amount",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "maxServiceFee",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "owner",
        "type": "bytes",
        "internalType": "bytes"
      },
      {
        "name": "subaccount",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ],
    "outputs": [
      {
        "name": "withdrawalId",
        "type": "uint256",
        "internalType": "uint256"
      }
    ],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "eip712Domain",
    "inputs": [],
    "outputs": [
      {
        "name": "fields",
        "type": "bytes1",
        "internalType": "bytes1"
      },
      {
        "name": "name",
        "type": "string",
        "internalType": "string"
      },
      {
        "name": "version",
        "type": "string",
        "internalType": "string"
      },
      {
        "name": "chainId",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "verifyingContract",
        "type": "address",
        "internalType": "address"
      },
      {
        "name": "salt",
        "type": "bytes32",
        "internalType": "bytes32"
      },
      {
        "name": "extensions",
        "type": "uint256[]",
        "internalType": "uint256[]"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "getWithdrawal",
    "inputs": [
      {
        "name": "withdrawalId",
        "type": "uint256",
        "internalType": "uint256"
      }
    ],
    "outputs": [
      {
        "name": "",
        "type": "tuple",
        "internalType": "struct IMultiTokenBridge.Withdrawal",
        "components": [
          {
            "name": "assetId",
            "type": "bytes32",
            "internalType": "bytes32"
          },
          {
            "name": "requester",
            "type": "address",
            "internalType": "address"
          },
          {
            "name": "amount",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "maxServiceFee",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "chargedServiceFee",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "amountOut",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "owner",
            "type": "bytes",
            "internalType": "bytes"
          },
          {
            "name": "subaccount",
            "type": "bytes32",
            "internalType": "bytes32"
          },
          {
            "name": "status",
            "type": "uint8",
            "internalType": "enum IMultiTokenBridge.WithdrawalStatus"
          }
        ]
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "globalDepositMintsPaused",
    "inputs": [],
    "outputs": [
      {
        "name": "",
        "type": "bool",
        "internalType": "bool"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "globalEpoch",
    "inputs": [],
    "outputs": [
      {
        "name": "",
        "type": "uint256",
        "internalType": "uint256"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "globalWithdrawalsPaused",
    "inputs": [],
    "outputs": [
      {
        "name": "",
        "type": "bool",
        "internalType": "bool"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "isDepositProcessed",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      },
      {
        "name": "depositId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ],
    "outputs": [
      {
        "name": "",
        "type": "bool",
        "internalType": "bool"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "mintDepositWithAuthorization",
    "inputs": [
      {
        "name": "authorization",
        "type": "tuple",
        "internalType": "struct IMultiTokenBridge.MintAuthorization",
        "components": [
          {
            "name": "assetId",
            "type": "bytes32",
            "internalType": "bytes32"
          },
          {
            "name": "depositId",
            "type": "bytes32",
            "internalType": "bytes32"
          },
          {
            "name": "recipient",
            "type": "address",
            "internalType": "address"
          },
          {
            "name": "grossAmount",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "maxServiceFee",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "chargedServiceFee",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "deadline",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "globalEpoch",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "assetEpoch",
            "type": "uint256",
            "internalType": "uint256"
          }
        ]
      },
      {
        "name": "signature",
        "type": "bytes",
        "internalType": "bytes"
      }
    ],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "nextWithdrawalId",
    "inputs": [],
    "outputs": [
      {
        "name": "",
        "type": "uint256",
        "internalType": "uint256"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "pauseAllDepositMints",
    "inputs": [],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "pauseAllWithdrawals",
    "inputs": [],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "pauseAssetDepositMints",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "pauseAssetWithdrawals",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "registerAsset",
    "inputs": [
      {
        "name": "registration",
        "type": "tuple",
        "internalType": "struct IMultiTokenBridge.AssetRegistration",
        "components": [
          {
            "name": "assetId",
            "type": "bytes32",
            "internalType": "bytes32"
          },
          {
            "name": "tokenName",
            "type": "string",
            "internalType": "string"
          },
          {
            "name": "tokenSymbol",
            "type": "string",
            "internalType": "string"
          },
          {
            "name": "tokenDecimals",
            "type": "uint8",
            "internalType": "uint8"
          },
          {
            "name": "perDepositLimit",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "mintWindowLimit",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "mintWindowDuration",
            "type": "uint64",
            "internalType": "uint64"
          },
          {
            "name": "minServiceFee",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "maxServiceFee",
            "type": "uint256",
            "internalType": "uint256"
          },
          {
            "name": "initialServiceFee",
            "type": "uint256",
            "internalType": "uint256"
          }
        ]
      }
    ],
    "outputs": [
      {
        "name": "tokenAddress",
        "type": "address",
        "internalType": "address"
      }
    ],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "rotateBridgeSigner",
    "inputs": [
      {
        "name": "newSigner",
        "type": "address",
        "internalType": "address"
      }
    ],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "rotateRuntimeAdministrator",
    "inputs": [
      {
        "name": "newAdministrator",
        "type": "address",
        "internalType": "address"
      }
    ],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "runtimeAdministrator",
    "inputs": [],
    "outputs": [
      {
        "name": "",
        "type": "address",
        "internalType": "address"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "setAssetServiceFee",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      },
      {
        "name": "newServiceFee",
        "type": "uint256",
        "internalType": "uint256"
      }
    ],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "tokenForAsset",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ],
    "outputs": [
      {
        "name": "",
        "type": "address",
        "internalType": "address"
      }
    ],
    "stateMutability": "view"
  },
  {
    "type": "function",
    "name": "unpauseAllDepositMints",
    "inputs": [],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "unpauseAllWithdrawals",
    "inputs": [],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "unpauseAssetDepositMints",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "function",
    "name": "unpauseAssetWithdrawals",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ],
    "outputs": [],
    "stateMutability": "nonpayable"
  },
  {
    "type": "event",
    "name": "AssetDepositMintsPaused",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "indexed": true,
        "internalType": "bytes32"
      },
      {
        "name": "caller",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "AssetDepositMintsUnpaused",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "indexed": true,
        "internalType": "bytes32"
      },
      {
        "name": "caller",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "AssetEpochChanged",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "indexed": true,
        "internalType": "bytes32"
      },
      {
        "name": "caller",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      },
      {
        "name": "previousEpoch",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "newEpoch",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "AssetRegistered",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "indexed": true,
        "internalType": "bytes32"
      },
      {
        "name": "token",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      },
      {
        "name": "tokenName",
        "type": "string",
        "indexed": false,
        "internalType": "string"
      },
      {
        "name": "tokenSymbol",
        "type": "string",
        "indexed": false,
        "internalType": "string"
      },
      {
        "name": "tokenDecimals",
        "type": "uint8",
        "indexed": false,
        "internalType": "uint8"
      },
      {
        "name": "perDepositLimit",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "mintWindowLimit",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "mintWindowDuration",
        "type": "uint64",
        "indexed": false,
        "internalType": "uint64"
      },
      {
        "name": "minServiceFee",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "maxServiceFee",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "initialServiceFee",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "AssetServiceFeeChanged",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "indexed": true,
        "internalType": "bytes32"
      },
      {
        "name": "caller",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      },
      {
        "name": "previousFee",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "newFee",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "AssetWithdrawalsPaused",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "indexed": true,
        "internalType": "bytes32"
      },
      {
        "name": "caller",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "AssetWithdrawalsUnpaused",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "indexed": true,
        "internalType": "bytes32"
      },
      {
        "name": "caller",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "BridgeSignerChanged",
    "inputs": [
      {
        "name": "previousSigner",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      },
      {
        "name": "newSigner",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "DepositMinted",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "indexed": true,
        "internalType": "bytes32"
      },
      {
        "name": "depositId",
        "type": "bytes32",
        "indexed": true,
        "internalType": "bytes32"
      },
      {
        "name": "recipient",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      },
      {
        "name": "authorizationDigest",
        "type": "bytes32",
        "indexed": false,
        "internalType": "bytes32"
      },
      {
        "name": "grossAmount",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "serviceFee",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "mintedAmount",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "EIP712DomainChanged",
    "inputs": [],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "GlobalDepositMintsPaused",
    "inputs": [
      {
        "name": "caller",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "GlobalDepositMintsUnpaused",
    "inputs": [
      {
        "name": "caller",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "GlobalEpochChanged",
    "inputs": [
      {
        "name": "caller",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      },
      {
        "name": "previousEpoch",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "newEpoch",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "GlobalWithdrawalsPaused",
    "inputs": [
      {
        "name": "caller",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "GlobalWithdrawalsUnpaused",
    "inputs": [
      {
        "name": "caller",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "RuntimeAdministratorChanged",
    "inputs": [
      {
        "name": "previousAdministrator",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      },
      {
        "name": "newAdministrator",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      }
    ],
    "anonymous": false
  },
  {
    "type": "event",
    "name": "WithdrawalCommitted",
    "inputs": [
      {
        "name": "withdrawalId",
        "type": "uint256",
        "indexed": true,
        "internalType": "uint256"
      },
      {
        "name": "assetId",
        "type": "bytes32",
        "indexed": true,
        "internalType": "bytes32"
      },
      {
        "name": "requester",
        "type": "address",
        "indexed": true,
        "internalType": "address"
      },
      {
        "name": "amount",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "maxServiceFee",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "chargedServiceFee",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "amountOut",
        "type": "uint256",
        "indexed": false,
        "internalType": "uint256"
      },
      {
        "name": "owner",
        "type": "bytes",
        "indexed": false,
        "internalType": "bytes"
      },
      {
        "name": "subaccount",
        "type": "bytes32",
        "indexed": false,
        "internalType": "bytes32"
      }
    ],
    "anonymous": false
  },
  {
    "type": "error",
    "name": "AssetAlreadyRegistered",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ]
  },
  {
    "type": "error",
    "name": "AssetEpochMismatch",
    "inputs": [
      {
        "name": "suppliedEpoch",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "currentEpoch",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "AssetLimitReached",
    "inputs": [
      {
        "name": "maximumAssets",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "BlockTimestampExceedsU64",
    "inputs": [
      {
        "name": "timestamp",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "BridgeSignerAlreadyRetired",
    "inputs": [
      {
        "name": "signer",
        "type": "address",
        "internalType": "address"
      }
    ]
  },
  {
    "type": "error",
    "name": "BridgeSignerRotationRequired",
    "inputs": []
  },
  {
    "type": "error",
    "name": "DepositAlreadyProcessed",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      },
      {
        "name": "depositId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ]
  },
  {
    "type": "error",
    "name": "DepositMintLimitExceeded",
    "inputs": [
      {
        "name": "mintAmount",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "limit",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "DepositMintsArePaused",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ]
  },
  {
    "type": "error",
    "name": "EmptyTokenMetadata",
    "inputs": []
  },
  {
    "type": "error",
    "name": "GlobalEpochMismatch",
    "inputs": [
      {
        "name": "suppliedEpoch",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "currentEpoch",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "InvalidAmount",
    "inputs": [
      {
        "name": "amount",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "InvalidMintAuthorizationSignature",
    "inputs": []
  },
  {
    "type": "error",
    "name": "InvalidMintRecipient",
    "inputs": [
      {
        "name": "recipient",
        "type": "address",
        "internalType": "address"
      }
    ]
  },
  {
    "type": "error",
    "name": "InvalidMintWindowDuration",
    "inputs": [
      {
        "name": "suppliedDuration",
        "type": "uint64",
        "internalType": "uint64"
      },
      {
        "name": "minimumDuration",
        "type": "uint64",
        "internalType": "uint64"
      },
      {
        "name": "maximumDuration",
        "type": "uint64",
        "internalType": "uint64"
      }
    ]
  },
  {
    "type": "error",
    "name": "InvalidPrincipal",
    "inputs": [
      {
        "name": "owner",
        "type": "bytes",
        "internalType": "bytes"
      }
    ]
  },
  {
    "type": "error",
    "name": "InvalidServiceFee",
    "inputs": [
      {
        "name": "serviceFee",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "maximumServiceFee",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "InvalidShortString",
    "inputs": []
  },
  {
    "type": "error",
    "name": "MintAuthorizationDeadlineTooFar",
    "inputs": [
      {
        "name": "deadline",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "maximumDeadline",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "MintAuthorizationExpired",
    "inputs": [
      {
        "name": "currentTimestamp",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "deadline",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "MintWindowLimitExceeded",
    "inputs": [
      {
        "name": "requestedAmount",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "availableAmount",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "MultipleWithdrawalsInTransaction",
    "inputs": []
  },
  {
    "type": "error",
    "name": "RoleAddressesMustDiffer",
    "inputs": []
  },
  {
    "type": "error",
    "name": "ServiceFeeExceedsUserMaximum",
    "inputs": [
      {
        "name": "serviceFee",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "userMaximum",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "StringTooLong",
    "inputs": [
      {
        "name": "str",
        "type": "string",
        "internalType": "string"
      }
    ]
  },
  {
    "type": "error",
    "name": "TimelockCandidateCodeHashMismatch",
    "inputs": [
      {
        "name": "candidate",
        "type": "address",
        "internalType": "address"
      },
      {
        "name": "actualCodeHash",
        "type": "bytes32",
        "internalType": "bytes32"
      },
      {
        "name": "expectedCodeHash",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ]
  },
  {
    "type": "error",
    "name": "TimelockCandidateDelayTooLong",
    "inputs": [
      {
        "name": "candidate",
        "type": "address",
        "internalType": "address"
      },
      {
        "name": "suppliedDelay",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "maximumDelay",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "TimelockCandidateDelayTooShort",
    "inputs": [
      {
        "name": "candidate",
        "type": "address",
        "internalType": "address"
      },
      {
        "name": "suppliedDelay",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "minimumDelay",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "TimelockCandidateHasNoCode",
    "inputs": [
      {
        "name": "candidate",
        "type": "address",
        "internalType": "address"
      }
    ]
  },
  {
    "type": "error",
    "name": "TimelockCandidateHasPendingOperations",
    "inputs": [
      {
        "name": "candidate",
        "type": "address",
        "internalType": "address"
      },
      {
        "name": "pendingOperationCount",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "TimelockCandidateIntrospectionFailed",
    "inputs": [
      {
        "name": "candidate",
        "type": "address",
        "internalType": "address"
      }
    ]
  },
  {
    "type": "error",
    "name": "TimelockCandidateInvalidRoleMember",
    "inputs": [
      {
        "name": "candidate",
        "type": "address",
        "internalType": "address"
      },
      {
        "name": "role",
        "type": "bytes32",
        "internalType": "bytes32"
      },
      {
        "name": "member",
        "type": "address",
        "internalType": "address"
      }
    ]
  },
  {
    "type": "error",
    "name": "TimelockCandidateMissingSelfAdmin",
    "inputs": [
      {
        "name": "candidate",
        "type": "address",
        "internalType": "address"
      }
    ]
  },
  {
    "type": "error",
    "name": "TokenAlreadyRegistered",
    "inputs": [
      {
        "name": "token",
        "type": "address",
        "internalType": "address"
      }
    ]
  },
  {
    "type": "error",
    "name": "TokenMetadataTooLong",
    "inputs": [
      {
        "name": "nameLength",
        "type": "uint256",
        "internalType": "uint256"
      },
      {
        "name": "symbolLength",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "TokenTransferFailed",
    "inputs": []
  },
  {
    "type": "error",
    "name": "UnauthorizedBaseAdmin",
    "inputs": [
      {
        "name": "caller",
        "type": "address",
        "internalType": "address"
      }
    ]
  },
  {
    "type": "error",
    "name": "UnauthorizedRuntimeAdministrator",
    "inputs": [
      {
        "name": "caller",
        "type": "address",
        "internalType": "address"
      }
    ]
  },
  {
    "type": "error",
    "name": "UnknownAsset",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ]
  },
  {
    "type": "error",
    "name": "ValueExceedsU128",
    "inputs": [
      {
        "name": "value",
        "type": "uint256",
        "internalType": "uint256"
      }
    ]
  },
  {
    "type": "error",
    "name": "WithdrawalsArePaused",
    "inputs": [
      {
        "name": "assetId",
        "type": "bytes32",
        "internalType": "bytes32"
      }
    ]
  },
  {
    "type": "error",
    "name": "ZeroAddress",
    "inputs": []
  },
  {
    "type": "error",
    "name": "ZeroAssetId",
    "inputs": []
  }
] as const
