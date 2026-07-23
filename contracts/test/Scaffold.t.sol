// contracts/test: prove the Phase 1B Bridge creates its bound bSNS deployment unit.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {BSNS} from "../src/BSNS.sol";
import {Bridge} from "../src/Bridge.sol";
import {BridgeTimelockController} from "../src/BridgeTimelockController.sol";

contract ScaffoldTest {
    function testDeploysStandaloneBSNS() public {
        BSNS token = new BSNS("kinic", "KINIC", 8, address(this));
        assert(address(token).code.length > 0);
    }

    function testBridgeDeploysBoundBSNS() public {
        address[] memory operators = new address[](1);
        operators[0] = address(0x33);
        address[] memory cancellers = new address[](1);
        cancellers[0] = address(0x44);
        BridgeTimelockController timelock = new BridgeTimelockController(72 hours, operators, cancellers, operators);
        Bridge bridge = new Bridge(
            "kinic",
            "KINIC",
            8,
            address(0x11),
            address(0x22),
            address(timelock),
            address(timelock).codehash,
            100,
            200,
            1 hours,
            10,
            1
        );
        assert(address(bridge).code.length > 0);
        assert(address(bridge.bsns()).code.length > 0);
        assert(bridge.bsns().bridge() == address(bridge));
    }
}
